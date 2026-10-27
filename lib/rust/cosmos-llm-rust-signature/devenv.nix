{ pkgs, lib, config, inputs, ... }:

{
  env.PROJECT_NAME = "cosmos-llm-signature";

  packages = with pkgs; [ git ];

  languages.rust.enable = true;

  enterShell = ''
    echo "cosmos-llm-signature dev environment"
    rustc --version
    cargo --version
  '';

  enterTest = ''
    cargo test
    cargo test --features cli
  '';
}
