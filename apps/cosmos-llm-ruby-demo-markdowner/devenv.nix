{ pkgs, lib, config, inputs, ... }:

{
  packages = with pkgs; [ git libyaml openssl ];

  languages.ruby.enable = true;

  enterShell = ''

  '';
}
