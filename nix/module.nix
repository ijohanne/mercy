{ config, lib, pkgs, ... }:

let
  cfg = config.services.mercy;

  startScript = pkgs.writeShellScript "mercy-start" ''
    set -euo pipefail
    export MERCY_AUTH_TOKEN="$(cat ${cfg.authTokenFile})"
    export MERCY_TB_EMAIL="$(cat ${cfg.tbEmailFile})"
    export MERCY_TB_PASSWORD="$(cat ${cfg.tbPasswordFile})"
    exec ${pkgs.xvfb-run}/bin/xvfb-run -s '-screen 0 1920x1080x24' ${cfg.package}/bin/mercy
  '';
in
{
  options.services.mercy = {
    enable = lib.mkEnableOption "Mercy mercenary exchange locator";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The mercy package to use";
    };

    kingdoms = lib.mkOption {
      type = lib.types.str;
      example = "109,110,112,113,114";
      description = "Comma-separated list of kingdom IDs to scan";
    };

    listenPort = lib.mkOption {
      type = lib.types.port;
      default = 8090;
      description = "Port for the REST API to listen on";
    };

    authTokenFile = lib.mkOption {
      type = lib.types.path;
      description = "File containing the API auth token";
    };

    tbEmailFile = lib.mkOption {
      type = lib.types.path;
      description = "File containing Total Battle login email";
    };

    tbPasswordFile = lib.mkOption {
      type = lib.types.path;
      description = "File containing Total Battle login password";
    };

    chromiumPackage = lib.mkOption {
      type = lib.types.package;
      default = pkgs.chromium;
      defaultText = lib.literalExpression "pkgs.chromium";
      description = "Chromium package to use for headless browsing";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.mercy = {
      description = "Mercy - Mercenary Exchange Locator";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];

      environment = {
        MERCY_KINGDOMS = cfg.kingdoms;
        MERCY_LISTEN_ADDR = "0.0.0.0:${toString cfg.listenPort}";
        MERCY_CHROMIUM_PATH = "${cfg.chromiumPackage}/bin/chromium";
      };

      serviceConfig = {
        Type = "simple";
        DynamicUser = true;
        StateDirectory = "mercy";
        WorkingDirectory = "${cfg.package}/share/mercy";

        ExecStart = startScript;

        Restart = "on-failure";
        RestartSec = 10;

        # Security hardening
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictNamespaces = true;
        RestrictSUIDSGID = true;
        MemoryDenyWriteExecute = false; # Chromium needs this
        ReadWritePaths = [ "/var/lib/mercy" ];
      };
    };
  };
}
