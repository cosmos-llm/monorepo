{ pkgs, lib, config, inputs, ... }:

{
  packages = with pkgs; [ git ];

  languages.javascript.enable = true;
  languages.javascript.npm.enable = true;

}
