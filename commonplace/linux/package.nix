# SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

{
  gtk4,
  gtksourceview5,
  lib,
  libadwaita,
  pkg-config,
  rustPlatform,
  wrapGAppsHook4,
}:

let
  desktopEntry = "linux/data/systems.thoughtfull.Commonplace.desktop";
  workspace = ../.;
in
rustPlatform.buildRustPackage {
  # keep-sorted start block=yes newline_separated=yes
  buildInputs = [
    # keep-sorted start
    gtk4
    gtksourceview5
    libadwaita
    # keep-sorted end
  ];

  cargoBuildFlags = [
    "--package"
    "commonplace-gtk"
  ];

  cargoLock.lockFile = workspace + "/Cargo.lock";

  # The app has no tests of its own, and the core's tests need no display.
  cargoTestFlags = [
    "--package"
    "commonplace-core"
  ];

  inherit ((lib.importTOML (workspace + "/Cargo.toml")).workspace.package) version;

  meta = {
    # keep-sorted start
    description = "Personal knowledge management with memos, actions, curios, and zettels";
    license = lib.licenses.mpl20;
    mainProgram = "commonplace";
    platforms = lib.platforms.linux;
    # keep-sorted end
  };

  nativeBuildInputs = [
    # keep-sorted start
    pkg-config
    wrapGAppsHook4
    # keep-sorted end
  ];

  pname = "commonplace";

  postInstall = "install -Dm644 ${desktopEntry} -t $out/share/applications";

  src = lib.fileset.toSource {
    root = workspace;
    fileset = lib.fileset.unions [
      # keep-sorted start
      (workspace + "/Cargo.lock")
      (workspace + "/Cargo.toml")
      (workspace + "/core")
      (workspace + "/linux/Cargo.toml")
      (workspace + "/linux/data")
      (workspace + "/linux/src")
      # keep-sorted end
    ];
  };
  # keep-sorted end
}
