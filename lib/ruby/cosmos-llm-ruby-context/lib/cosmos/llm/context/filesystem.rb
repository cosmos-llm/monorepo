# frozen_string_literal: true

module Cosmos
  module Llm
    module Context
      # Compatibility alias for Cosmos::Llm::VirtualFilesystem::Filesystem
      #
      # This class provides backward compatibility by aliasing the extracted
      # Filesystem class from the cosmos-llm-virtual-filesystem gem.
      #
      # @deprecated Use Cosmos::Llm::VirtualFilesystem::Filesystem instead
      # @see Cosmos::Llm::VirtualFilesystem::Filesystem
      Filesystem = Cosmos::Llm::VirtualFilesystem::Filesystem

      # Compatibility alias for Cosmos::Llm::VirtualFilesystem::VirtualFile
      #
      # This class provides backward compatibility by aliasing the extracted
      # VirtualFile class from the cosmos-llm-virtual-filesystem gem.
      #
      # @deprecated Use Cosmos::Llm::VirtualFilesystem::VirtualFile instead
      # @see Cosmos::Llm::VirtualFilesystem::VirtualFile
      VirtualFile = Cosmos::Llm::VirtualFilesystem::VirtualFile
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
