{ pkgs, lib, config, inputs, ... }:

{
  env.PROJECT_NAME = "cosmos-llm-predict";

  packages = with pkgs; [ git ];

  languages.rust.enable = true;

  enterShell = ''
    echo "cosmos-llm-predict dev environment"
    rustc --version
    cargo --version
  '';

  enterTest = ''
    cargo test
    cargo test --no-default-features
    cargo test --features cli
  '';
}
