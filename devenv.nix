# SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

{
  config,
  lib,
  pkgs,
  ...
}:

let
  androidAapt2 = "${androidHome}/build-tools/36.0.0/aapt2";
  androidHome = "${androidSdk.androidsdk}/libexec/android-sdk";
  androidSdk =
    (pkgs.androidenv.override {
      # keep-sorted start block=yes
      licenseAccepted = true;
      # keep-sorted end
    }).composeAndroidPackages
      {
        # keep-sorted start block=yes
        buildToolsVersions = [ "36.0.0" ];
        cmakeVersions = [ ];
        includeNDK = true;
        ndkVersions = [ "28.2.13676358" ];
        platformVersions = [ "36" ];
        toolsVersion = null;
        # keep-sorted end
      };
  gtkSchemas = "${pkgs.gtk4}/share/gsettings-schemas/${pkgs.gtk4.name}";
  kotlinFormatter = pkgs.ktlint.override {
    # keep-sorted start block=yes
    jre_headless = pkgs.jdk17;
    # keep-sorted end
  };
  vendored = "^\\.config/vale/styles/(Google|write-good)/";
in
{
  # keep-sorted start block=yes newline_separated=yes
  # Nix builds wrap GTK apps to find GSettings schemas; apps run from the shell need them here.
  enterShell = "export XDG_DATA_DIRS=${gtkSchemas}:$XDG_DATA_DIRS";

  env.VALE_CONFIG_PATH = "${config.devenv.root}/.config/vale/vale.ini";

  git-hooks.hooks = {
    # keep-sorted start block=yes
    actionlint.enable = true;
    check-added-large-files.enable = true;
    check-case-conflicts.enable = true;
    check-executables-have-shebangs.enable = true;
    check-merge-conflicts.enable = true;
    check-shebang-scripts-are-executable.enable = true;
    check-symlinks.enable = true;
    check-vcs-permalinks.enable = true;
    clippy = {
      enable = true;
      settings.denyWarnings = true;
    };
    deadnix.enable = true;
    detect-private-keys.enable = true;
    editorconfig-checker = {
      enable = true;
      excludes = [
        vendored
        "^devenv\\.lock$"
      ];
    };
    end-of-file-fixer = {
      enable = true;
      # devenv writes the lock file without a trailing newline.
      excludes = [
        vendored
        "^devenv\\.lock$"
      ];
    };
    fix-byte-order-marker.enable = true;
    forbid-new-submodules.enable = true;
    gitlint = {
      enable = true;
      # --msg-filename ends the stock entry, so args can't add the config flag.
      entry = lib.mkForce (
        lib.concatStringsSep " " [
          (lib.getExe config.git-hooks.hooks.gitlint.package)
          "--config .config/gitlint --staged --msg-filename"
        ]
      );
    };
    keep-sorted = {
      # The flag replaces every default option, so it repeats them all with only case changed.
      args = [
        "--default-options"
        "allow_yaml_lists=yes case=no group=yes order=asc remove_duplicates=yes sticky_comments=yes"
      ];
      enable = true;
    };
    ktlint = {
      # keep-sorted start block=yes
      enable = true;
      entry = "${lib.getExe kotlinFormatter} --editorconfig=.config/ktlint.editorconfig";
      files = "\\.kts?$";
      name = "ktlint";
      # keep-sorted end
    };
    license-header = {
      enable = true;
      excludes = [ "^commonplace/vendor/" ];
      entry = lib.getExe (
        pkgs.writeShellApplication {
          name = "license-header";
          runtimeInputs = [ pkgs.gawk ];
          text = ''
            gawk '
              BEGINFILE { found = 0 }
              FNR > 5 { nextfile }
              /SPDX-License-Identifier: MPL-2\.0-no-copyleft-exception/ { found = 1 }
              ENDFILE { if (!found) { printf "%s: no license header\n", FILENAME; bad = 1 } }
              END { exit bad }
            ' "$@"
          '';
        }
      );
      name = "license-header";
      types_or = [
        "kotlin"
        "nix"
        "rust"
        "shell"
        "svg"
        "xml"
      ];
    };
    # No stock hook checks line length in every file type, and nixfmt doesn't fail on long lines.
    line-length = {
      enable = true;
      entry = lib.getExe (
        pkgs.writeShellApplication {
          name = "line-length";
          runtimeInputs = [ pkgs.gawk ];
          text = ''
            LC_ALL=C.UTF-8 gawk '
              length > 100 { printf "%s:%d: %d characters\n", FILENAME, FNR, length; bad = 1 }
              END { exit bad }
            ' "$@"
          '';
        }
      );
      excludes = [
        vendored
        "^devenv\\.lock$"
      ];
      name = "line-length";
      types = [ "text" ];
    };
    markdownlint = {
      enable = true;
      # The stock entry passes a config generated from Nix; use the file in .config instead.
      entry = lib.mkForce (
        lib.concatStringsSep " " [
          (lib.getExe config.git-hooks.hooks.markdownlint.package)
          "--config .config/markdownlint.yaml"
        ]
      );
      excludes = [ vendored ];
    };
    mdformat = {
      args = [
        "--wrap"
        "100"
      ];
      enable = true;
      excludes = [ vendored ];
      package = pkgs.mdformat.withPlugins (ps: [ ps.mdformat-gfm ]);
    };
    mixed-line-endings = {
      args = [ "--fix=lf" ];
      enable = true;
    };
    nixfmt = {
      enable = true;
      settings.width = 100;
    };
    ripsecrets.enable = true;
    rustfmt = {
      enable = true;
      settings.config-path = ".config/rustfmt.toml";
    };
    sort-file-contents = {
      enable = true;
      files = lib.concatStrings [
        "^(\\.gitignore"
        "|\\.config/vale/styles/config/vocabularies/[^/]+/(accept|reject)\\.txt)$"
      ];
      settings.ignore-case = true;
    };
    statix.enable = true;
    taplo = {
      args = [
        "--config"
        ".config/taplo.toml"
      ];
      enable = true;
    };
    trim-trailing-whitespace = {
      args = [ "--markdown-linebreak-ext=md" ];
      enable = true;
      excludes = [ vendored ];
    };
    typos = {
      enable = true;
      excludes = [ vendored ];
    };
    vale = {
      enable = true;
      excludes = [ vendored ];
      settings.configPath = ".config/vale/vale.ini";
      types = [ "markdown" ];
    };
    vale-config = {
      enable = true;
      entry = "${lib.getExe pkgs.vale} --config .config/vale/vale.ini ls-config";
      files = "^\\.config/vale/";
      name = "vale-config";
      pass_filenames = false;
    };
    yaml-extension = {
      enable = true;
      entry = lib.getExe (
        pkgs.writeShellApplication {
          name = "yaml-extension";
          text = ''
            status=0
            for file in "$@"; do
              case "$file" in
                .config/vale/*.yaml)
                  echo "$file: Vale ignores rule files unless they end in .yml"
                  status=1
                  ;;
                .config/vale/*) ;;
                *.yml)
                  echo "$file: use the .yaml extension"
                  status=1
                  ;;
              esac
            done
            exit "$status"
          '';
        }
      );
      files = "\\.ya?ml$";
      name = "yaml-extension";
    };
    yamlfmt = {
      enable = true;
      excludes = [ vendored ];
      settings = {
        configPath = ".config/yamlfmt.yaml";
        lint-only = false;
      };
    };
    yamllint = {
      enable = true;
      excludes = [ vendored ];
      settings.configPath = ".config/yamllint.yaml";
    };
    zizmor.enable = true;
    # keep-sorted end
  };

  git-hooks.settings.rust.cargoManifestPath = "commonplace/Cargo.toml";

  languages.rust.enable = true;

  packages = [
    # keep-sorted start
    kotlinFormatter
    pkgs.gtk4
    pkgs.gtksourceview5
    pkgs.libadwaita
    pkgs.lychee
    pkgs.pkg-config
    pkgs.vale
    # keep-sorted end
  ];

  profiles.android.module = {
    # keep-sorted start block=yes
    env = {
      # keep-sorted start block=yes
      ANDROID_HOME = androidHome;
      ANDROID_NDK_HOME = "${androidHome}/ndk/28.2.13676358";
      GRADLE_OPTS = "-Dorg.gradle.project.android.aapt2FromMavenOverride=${androidAapt2}";
      JAVA_HOME = "${pkgs.jdk17}";
      # keep-sorted end
    };
    languages.rust = {
      # keep-sorted start block=yes
      channel = "stable";
      targets = [
        # keep-sorted start
        "aarch64-linux-android"
        "x86_64-linux-android"
        # keep-sorted end
      ];
      # keep-sorted end
    };
    packages = [
      # keep-sorted start
      androidSdk.androidsdk
      pkgs.cargo-ndk
      pkgs.gradle
      pkgs.jdk17
      pkgs.librsvg
      # keep-sorted end
    ];
    # keep-sorted end
  };

  scripts.commonplace-test-capture = {
    description = "Capture into the test copy: commonplace-test-capture memo|action|journal";
    # Without a running test copy, the capture command would start one on the everyday library.
    exec = ''
      set -eu
      id=systems.thoughtfull.CommonplaceTest
      if ! busctl --user status "$id" >/dev/null 2>&1; then
        echo "commonplace-test-capture: start commonplace-test first" >&2
        exit 1
      fi
      COMMONPLACE_APP_ID=$id ${config.devenv.root}/commonplace/target/debug/commonplace capture "$@"
    '';
  };

  scripts.commonplace-test = {
    description = "Run on $COMMONPLACE_LIBRARY or fresh samples, with optional --date YYYY-MM-DD";
    # The separate app ID keeps the test copy from handing its work to an everyday copy.
    exec = ''
      set -eu
      sample_date=""
      while [ "$#" -gt 0 ]; do
        case "$1" in
          --date)
            if [ "$#" -lt 2 ] || [ -z "$2" ] || [ -n "$sample_date" ]; then
              echo "commonplace-test: use --date YYYY-MM-DD once" >&2
              exit 2
            fi
            sample_date=$2
            shift 2
            ;;
          --date=*)
            if [ -z "''${1#--date=}" ] || [ -n "$sample_date" ]; then
              echo "commonplace-test: use --date YYYY-MM-DD once" >&2
              exit 2
            fi
            sample_date=''${1#--date=}
            shift
            ;;
          -h|--help)
            echo "Usage: commonplace-test [--date YYYY-MM-DD]"
            echo "Generate samples at noon UTC on that date, then run with the real clock."
            exit 0
            ;;
          *)
            echo "commonplace-test: unknown argument: $1" >&2
            echo "Usage: commonplace-test [--date YYYY-MM-DD]" >&2
            exit 2
            ;;
        esac
      done
      manifest=${config.devenv.root}/commonplace/Cargo.toml
      if [ -n "''${COMMONPLACE_LIBRARY:-}" ]; then
        if [ -n "$sample_date" ]; then
          echo "commonplace-test: --date creates fresh samples; unset COMMONPLACE_LIBRARY" >&2
          exit 2
        fi
        if [ ! -d "$COMMONPLACE_LIBRARY" ]; then
          echo "commonplace-test: no library at $COMMONPLACE_LIBRARY" >&2
          exit 1
        fi
        echo "commonplace-test: running on $COMMONPLACE_LIBRARY" >&2
      else
        dir=$(mktemp -d -t commonplace-test.XXXXXX)
        trap 'rm -rf "$dir"' EXIT
        set -- "$dir/library"
        if [ -n "$sample_date" ]; then
          set -- "$@" --date "$sample_date"
        fi
        cargo run --quiet --manifest-path "$manifest" --example test_library -- "$@"
        export COMMONPLACE_LIBRARY="$dir/library"
      fi
      COMMONPLACE_APP_ID=systems.thoughtfull.CommonplaceTest \
        cargo run --quiet --manifest-path "$manifest" --bin commonplace
    '';
  };
  # keep-sorted end
}
