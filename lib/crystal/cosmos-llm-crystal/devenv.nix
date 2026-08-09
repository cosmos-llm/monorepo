{ pkgs, lib, config, inputs, ... }:

{
  env.PROJECT_NAME = "cosmos-llm-crystal";

  packages = with pkgs; [ git openssl pkg-config ];

  languages.crystal.enable = true;

  enterShell = ''
    echo "cosmos-llm-crystal dev environment"
    crystal --version
  '';

  enterTest = ''
    crystal spec
  '';
}
