# frozen_string_literal: true

require 'cosmos/llm/tool'
require 'fileutils'
require 'open3'
require 'tmpdir'
require 'json'

module Cosmos
  module Llm
    module Tool
      module Preset
        # Exec preset tool (virtual filesystem + gvisor sandbox)
        #
        # Runs shell commands inside a persistent gvisor (runsc) container scoped to
        # a VirtualFilesystem instance. The container is started lazily on the first
        # call and kept alive until the Ruby process exits.
        #
        # Lifecycle:
        # 1. First call: a temp dir is created, the VFS is materialized into it,
        #    and a gvisor container is started with that dir as its root.
        # 2. Before each command: changed VFS files are written to the temp dir
        #    (inotify watches the container's working dir for writes back out).
        # 3. The command runs inside the container.
        # 4. inotify-detected changes in the container dir are synced back to the VFS
        #    (returned in result[:fs_changes] for the caller to commit via write tool).
        # 5. At_exit: container is killed and temp dir is removed.
        #
        # Requirements:
        # - `runsc` (gvisor) must be on PATH
        # - `inotifywait` (inotify-tools) must be on PATH for change detection
        #
        # @example
        #   tool = Cosmos::Llm::Tool::Preset.exec(filesystem)
        #   result = tool.call(command: 'ruby main.rb')
        #   puts result[:stdout]
        #   result[:fs_changes].each { |c| puts "#{c[:path]}: #{c[:content]}" }
        #
        # @param filesystem [Cosmos::Llm::VirtualFilesystem::Filesystem]
        # @param allowlist [Array<String>, nil] if provided, only commands whose argv[0]
        #   matches an entry are permitted. Pass nil to allow all commands.
        # @return [Cosmos::Llm::Tool::Definition]
        def self.exec(filesystem, allowlist: nil)
          container = Preset::ExecContainer.new(filesystem)

          Cosmos::Llm::Tool.define(:exec, register: false) do
            description 'Run a shell command inside a sandboxed gvisor container backed by the virtual filesystem'

            parameter :command,
                      type: :string,
                      required: true,
                      description: 'Shell command to run (e.g. "ruby main.rb", "ls -la src/")'

            parameter :timeout,
                      type: :number,
                      required: false,
                      description: 'Maximum seconds to wait for the command (default: 30)'

            execute do |params|
              command = params[:command]
              timeout_sec = params.fetch(:timeout, 30).to_i

              unless command.is_a?(String) && !command.empty?
                next { success: false, error: 'command is required and must be a non-empty string' }
              end

              if allowlist
                argv0 = command.split.first
                unless allowlist.include?(argv0)
                  next {
                    success: false,
                    error: "Command '#{argv0}' is not in the allowlist",
                    command: command,
                    allowlist: allowlist
                  }
                end
              end

              container.run(command, timeout_sec)
            end
          end
        end

        # Manages a persistent gvisor container instance tied to a VirtualFilesystem.
        #
        # One container per VFS instance. Started lazily, stopped at_exit.
        class ExecContainer
          RUNSC      = 'runsc'
          INOTIFY    = 'inotifywait'
          OCI_BUNDLE = 'cosmos-llm-exec'

          def initialize(filesystem)
            @filesystem  = filesystem
            @tmpdir      = nil
            @container_id = nil
            @vfs_snapshot = {}   # path => content at last sync
            @mutex       = Mutex.new
          end

          # Run a command in the container, syncing VFS before and collecting
          # file changes after.
          #
          # @param command [String]
          # @param timeout [Integer]
          # @return [Hash]
          def run(command, timeout)
            ensure_running

            @mutex.synchronize do
              sync_vfs_to_disk
              changes_before = snapshot_disk

              stdout, stderr, exit_code = exec_in_container(command, timeout)

              changes_after = snapshot_disk
              fs_changes    = diff_snapshots(changes_before, changes_after)

              {
                success: exit_code == 0,
                command: command,
                exit_code: exit_code,
                stdout: stdout,
                stderr: stderr,
                fs_changes: fs_changes
              }
            end
          rescue StandardError => e
            { success: false, error: e.message, command: command }
          end

          private

          def ensure_running
            return if @container_id

            @tmpdir = Dir.mktmpdir('cosmos-llm-exec-')
            materialize_vfs
            start_container

            at_exit { stop_container }
          end

          # Write every VFS file to the temp dir.
          def materialize_vfs
            @filesystem.all_files.each do |entry|
              abs = File.join(@tmpdir, entry[:path])
              FileUtils.mkdir_p(File.dirname(abs))
              File.write(abs, entry[:file].content || '')
              @vfs_snapshot[entry[:path]] = entry[:file].content || ''
            end
          end

          # Write only VFS files that differ from our last snapshot.
          def sync_vfs_to_disk
            @filesystem.all_files.each do |entry|
              path    = entry[:path]
              content = entry[:file].content || ''
              next if @vfs_snapshot[path] == content

              abs = File.join(@tmpdir, path)
              FileUtils.mkdir_p(File.dirname(abs))
              File.write(abs, content)
              @vfs_snapshot[path] = content
            end
          end

          # Snapshot current disk state of the working dir.
          # @return [Hash<String, String>] relative_path => content
          def snapshot_disk
            result = {}
            Dir.glob(File.join(@tmpdir, '**', '*'), File::FNM_DOTMATCH).each do |abs|
              next if File.directory?(abs)

              rel = abs.sub("#{@tmpdir}/", '')
              result[rel] = File.read(abs) rescue nil
            end
            result
          end

          # Compare two snapshots; return entries that were added or modified.
          # @return [Array<Hash>] [{path:, content:, change: :created|:modified}]
          def diff_snapshots(before, after)
            changes = []
            after.each do |path, content|
              next if content.nil?

              if !before.key?(path)
                changes << { path: path, content: content, change: :created }
              elsif before[path] != content
                changes << { path: path, content: content, change: :modified }
              end
            end
            changes
          end

          # Start a gvisor container using `runsc run`.
          # The container runs a long-lived sleep so it stays alive.
          def start_container
            @container_id = "cosmos-llm-#{Process.pid}-#{object_id}"
            bundle_dir    = build_oci_bundle

            # Start detached; we exec commands into it with `runsc exec`
            pid = spawn(
              RUNSC, 'run',
              '--rootless',
              '--network=none',
              '--bundle', bundle_dir,
              @container_id,
              in: :close, out: :close, err: :close
            )
            Process.detach(pid)

            # Brief wait for container to initialize
            sleep 0.3
          end

          # Execute a command inside the running container via `runsc exec`.
          # Uses inotifywait on a best-effort basis — falling back gracefully if unavailable.
          #
          # @return [Array(String, String, Integer)] stdout, stderr, exit_code
          def exec_in_container(command, timeout)
            cmd = [RUNSC, 'exec', '--rootless', @container_id, '/bin/sh', '-c', command]

            stdout_data = ''
            stderr_data = ''
            exit_code   = -1

            begin
              Open3.popen3(*cmd) do |_stdin, stdout, stderr, wait_thr|
                stdout_thread = Thread.new { stdout_data = stdout.read }
                stderr_thread = Thread.new { stderr_data = stderr.read }

                deadline = Time.now + timeout
                until wait_thr.join(0.1) || Time.now > deadline
                  # waiting
                end

                if Time.now > deadline && wait_thr.alive?
                  Process.kill('KILL', wait_thr.pid) rescue nil
                  stdout_data = "(timed out after #{timeout}s)\n"
                  exit_code   = -1
                else
                  exit_code = wait_thr.value.exitstatus || 0
                end

                stdout_thread.join(1)
                stderr_thread.join(1)
              end
            rescue Errno::ENOENT
              raise "gvisor (runsc) not found on PATH. Install gvisor to use the exec tool."
            end

            [stdout_data, stderr_data, exit_code]
          end

          # Build a minimal OCI bundle (config.json + rootfs) in a temp location.
          # @return [String] path to the bundle directory
          def build_oci_bundle
            bundle = File.join(@tmpdir, '.oci-bundle')
            rootfs = File.join(bundle, 'rootfs')
            FileUtils.mkdir_p(rootfs)

            # Minimal OCI runtime spec
            config = {
              ociVersion: '1.0.2',
              process: {
                terminal: false,
                user: { uid: 0, gid: 0 },
                args: ['/bin/sh', '-c', 'while true; do sleep 3600; done'],
                env: ['PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin'],
                cwd: '/workspace'
              },
              root: { path: 'rootfs', readonly: false },
              mounts: [
                { destination: '/workspace', type: 'bind', source: @tmpdir, options: %w[rbind rw] },
                { destination: '/proc',      type: 'proc', source: 'proc',  options: [] },
                { destination: '/tmp',       type: 'tmpfs', source: 'tmpfs', options: [] }
              ],
              linux: {
                namespaces: [
                  { type: 'pid' }, { type: 'ipc' }, { type: 'uts' }, { type: 'mount' }, { type: 'user' }
                ],
                uidMappings: [{ hostID: Process.uid, containerID: 0, size: 1 }],
                gidMappings: [{ hostID: Process.gid, containerID: 0, size: 1 }]
              }
            }

            File.write(File.join(bundle, 'config.json'), JSON.generate(config))
            bundle
          end

          def stop_container
            return unless @container_id

            system(RUNSC, 'kill',   '--rootless', @container_id, 'KILL', err: :close, out: :close)
            system(RUNSC, 'delete', '--rootless', @container_id,         err: :close, out: :close)
          rescue StandardError
            nil
          ensure
            FileUtils.rm_rf(@tmpdir) if @tmpdir
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
