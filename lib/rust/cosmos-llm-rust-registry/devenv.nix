{ pkgs, lib, config, inputs, ... }:

{
  env.PROJECT_NAME = "cosmos-llm-registry";

  packages = with pkgs; [ git ];

  languages.rust.enable = true;

  enterShell = ''
    echo "cosmos-llm-registry dev environment"
    rustc --version
    cargo --version
  '';

  enterTest = ''
    cargo test
  '';
}
