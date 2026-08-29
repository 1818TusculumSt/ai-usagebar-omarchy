{
  lib,
  makeWrapper,
  nasm,
  procps,
  rustPlatform,
  stdenv,
  xdg-utils,
}:

let
  cargoToml = builtins.fromTOML (builtins.readFile ../Cargo.toml);
  linuxRuntimePath = lib.makeBinPath [
    procps
    xdg-utils
  ];
in
rustPlatform.buildRustPackage {
  pname = "ai-usagebar-omarchy";
  version = cargoToml.package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../tests
      ../config.example.toml
      ../README.md
      ../LICENSE
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  # `claude_desktop::app` shells out to `/usr/bin/tar`, which the build sandbox
  # does not have. Skipped here rather than narrowing the module's `cfg`, so the
  # two security assertions it carries — archive permissions, and that a path
  # cannot carry a terminal escape out of a failure — keep running on Linux CI.
  checkFlags = [ "--skip=claude_desktop::app::tests" ];

  nativeBuildInputs =
    lib.optionals stdenv.hostPlatform.isx86_64 [ nasm ]
    ++ lib.optionals stdenv.hostPlatform.isLinux [ makeWrapper ];

  postInstall = ''
    install -Dm644 config.example.toml \
      "$out/share/ai-usagebar-omarchy/config.example.toml"
    install -Dm644 README.md \
      "$out/share/doc/ai-usagebar-omarchy/README.md"
    install -Dm644 LICENSE \
      "$out/share/licenses/ai-usagebar-omarchy/LICENSE"
  ''
  + lib.optionalString stdenv.hostPlatform.isLinux ''
    for program in ai-usagebar-omarchy ai-usagebar-omarchy-tui; do
      wrapProgram "$out/bin/$program" \
        --prefix PATH : "${linuxRuntimePath}"
    done
  '';

  meta = {
    description = "Omarchy/Waybar widgets + TUI for tracking multi-provider AI plan usage";
    homepage = "https://github.com/KyleLee/ai-usagebar-omarchy";
    license = lib.licenses.mit;
    mainProgram = "ai-usagebar-omarchy";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
      "x86_64-darwin"
      "aarch64-darwin"
    ];
  };
}
