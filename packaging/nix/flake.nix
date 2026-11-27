# Nix flake for the OpenDirectory Admin Center (x86_64-linux).
#
#   nix run 'github:Lin8x/OpenDirectory?dir=packaging/nix'
#   nix profile install 'github:Lin8x/OpenDirectory?dir=packaging/nix'
#
# NixOS: add this flake as an input and put
#   inputs.opendirectory.packages.x86_64-linux.default
# in environment.systemPackages.
#
# It wraps the release AppImage. sources.json pins the version and hash;
# packaging/render-manifests.sh writes a new one for each release (the
# release workflow attaches it in store-manifests.tar.gz).
{
  description = "OpenDirectory Admin Center: directory, Group Policy and endpoint management for every OS";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      src-info = builtins.fromJSON (builtins.readFile ./sources.json);
      pname = "opendirectory-admin-center";
      version = src-info.version;
      src = pkgs.fetchurl { inherit (src-info) url hash; };
      contents = pkgs.appimageTools.extract { inherit pname version src; };
      admin-center = pkgs.appimageTools.wrapType2 {
        inherit pname version src;
        extraInstallCommands = ''
          install -Dm644 "${contents}/OpenDirectory Admin Center.desktop" \
            $out/share/applications/${pname}.desktop
          install -Dm644 ${contents}/${pname}.png \
            $out/share/icons/hicolor/128x128/apps/${pname}.png
        '';
        meta = {
          description = "Directory, Group Policy and endpoint management for every OS";
          homepage = "https://github.com/Lin8x/OpenDirectory";
          mainProgram = pname;
          platforms = [ system ];
          sourceProvenance = [ pkgs.lib.sourceTypes.binaryNativeCode ];
        };
      };
    in
    {
      packages.${system} = {
        default = admin-center;
        inherit admin-center;
      };
      apps.${system}.default = {
        type = "app";
        program = "${admin-center}/bin/${pname}";
      };
    };
}
