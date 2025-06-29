{
  description = "markdown2pdf - Create PDF with Markdown files";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs = {
        nixpkgs.follows = "nixpkgs";
      };
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
    crane,
  }:
    flake-utils.lib.eachDefaultSystem
    (system: let
      overlays = [(import rust-overlay)];
      pkgs = import nixpkgs {
        inherit system overlays;
      };

      craneLib = crane.mkLib pkgs;

      rustSource = pkgs.lib.cleanSourceWith {
        src = ./.;
        filter = path: type:
          craneLib.filterCargoSources path type
          || pkgs.lib.hasInfix "/assets/" path;
      };

      # Function to build cargo artifacts for a specific profile
      mkCargoArtifacts = profile:
        craneLib.buildDepsOnly {
          src = rustSource;
          strictDeps = true;
          pname = "markdown2pdf-deps-${profile}";
          version = "0.1.3";
          CARGO_PROFILE = profile;
        };

      # Function to build markdown2pdf for a specific profile
      mkMarkdown2pdf = profile: let
        cargoArtifacts = mkCargoArtifacts profile;
      in
        craneLib.buildPackage {
          src = rustSource;
          strictDeps = true;
          inherit cargoArtifacts;
          doCheck = false;
          pname = "markdown2pdf";
          version = "0.1.3";
          CARGO_PROFILE = profile;
        };

      # Build artifacts and packages for both profiles
      debugCargoArtifacts = mkCargoArtifacts "dev";
      releaseCargoArtifacts = mkCargoArtifacts "release";

      markdown2pdf-debug = mkMarkdown2pdf "dev";
      markdown2pdf-release = mkMarkdown2pdf "release";

      rustVersion = pkgs.pkgsBuildHost.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      rustToolchain = rustVersion.override {
        extensions = ["rust-analyzer" "rust-src"];
      };

      nativeBuildInputs = with pkgs; [
        rustToolchain
        cargo-nextest
        cargo-audit
        cargo-watch
        cargo-deny
        cargo-machete
        bacon
        typos
      ];
    in
      with pkgs; {
        packages = {
          default = markdown2pdf-release;
          debug = markdown2pdf-debug;
          release = markdown2pdf-release;
        };

        apps.default = flake-utils.lib.mkApp {drv = markdown2pdf-release;};

        devShells.default = mkShell {
          inherit nativeBuildInputs;
          shellHook = ''
            echo "markdown2pdf development environment"
          '';
        };

        formatter = alejandra;
      });
}
