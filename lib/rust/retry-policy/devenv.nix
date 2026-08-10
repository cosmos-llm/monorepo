{ pkgs, lib, config, inputs, ... }:

{
  env.PROJECT_NAME = "retry-policy";

  packages = with pkgs; [ git ];

  languages.rust.enable = true;

  enterShell = ''
    echo "retry-policy dev environment"
    rustc --version
    cargo --version
  '';

  enterTest = ''
    cargo test
    cargo test --no-default-features
  '';
}
