{ pkgs, lib, config, inputs, ... }:

{
  env.PROJECT_NAME = "cosmos-llm-rust-context";

  packages = with pkgs; [ git ];

  languages.rust.enable = true;

  enterShell = ''
    echo "cosmos-llm-rust-context dev environment"
    rustc --version
    cargo --version
  '';

  enterTest = ''
    cargo test
  '';
}
