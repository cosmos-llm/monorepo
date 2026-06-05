{ pkgs, lib, config, inputs, ... }:

{
  env.PROJECT_NAME = "cosmos-llm-rust";

  packages = with pkgs; [ git zig openssl pkg-config ];

  languages.rust.enable = true;

  scripts.cargo-build-release.exec = ''
    cargo zigbuild --release --target x86_64-unknown-linux-musl "$@"
  '';

  enterShell = ''
    echo "cosmos-llm-rust dev environment"
    rustc --version
    cargo --version
  '';

  enterTest = ''
    cargo test
  '';
}
