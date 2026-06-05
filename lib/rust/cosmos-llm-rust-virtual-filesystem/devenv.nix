{ pkgs, lib, config, inputs, ... }:

{
  env.PROJECT_NAME = "cosmos-llm-rust-virtual-filesystem";

  packages = with pkgs; [ git ];

  languages.rust.enable = true;

  enterShell = ''
    echo "cosmos-llm-rust-virtual-filesystem dev environment"
    rustc --version
    cargo --version
  '';

  enterTest = ''
    cargo test
  '';
}
