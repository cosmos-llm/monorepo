{ pkgs, lib, config, inputs, ... }:

{
  # https://devenv.sh/packages/
  packages = with pkgs; [ git libyaml openssl ];

  languages.ruby.enable = true;


  enterShell = ''

  '';

}
