{ config, lib, pkgs, ... }:

let
  cfg = config.services.mercy;

  backendStartScript = pkgs.writeShellScript "mercy-backend-start" ''
    set -euo pipefail
    export MERCY_AUTH_TOKEN="$(cat ${cfg.authTokenFile})"
    export MERCY_TB_EMAIL="$(cat ${cfg.tbEmailFile})"
    export MERCY_TB_PASSWORD="$(cat ${cfg.tbPasswordFile})"
    exec ${pkgs.xvfb-run}/bin/xvfb-run -s '-screen 0 1920x1080x24' ${cfg.backendPackage}/bin/mercy
  '';

  frontendStartScript = pkgs.writeShellScript "mercy-frontend-start" ''
    set -euo pipefail
    export MERCY_AUTH_TOKEN="$(cat ${cfg.authTokenFile})"
    export MERCY_ADMIN_USER="$(cat ${cfg.adminUserFile})"
    export MERCY_ADMIN_PASSWORD="$(cat ${cfg.adminPasswordFile})"
    export MERCY_SESSION_SECRET="$(cat ${cfg.sessionSecretFile})"
    exec ${pkgs.bun}/bin/bun ${cfg.frontendPackage}/server.js
  '';
in
{
  options.services.mercy = {
    enable = lib.mkEnableOption "Mercy mercenary exchange locator";

    backendPackage = lib.mkOption {
      type = lib.types.package;
      description = "The mercy backend package to use";
    };

    frontendPackage = lib.mkOption {
      type = lib.types.package;
      description = "The mercy frontend package to use";
    };

    kingdoms = lib.mkOption {
      type = lib.types.str;
      example = "109,110,112,113,114";
      description = "Comma-separated list of kingdom IDs to scan";
    };

    backendPort = lib.mkOption {
      type = lib.types.port;
      default = 8090;
      description = "Port for the backend REST API";
    };

    frontendPort = lib.mkOption {
      type = lib.types.port;
      default = 3000;
      description = "Port for the frontend web UI";
    };

    authTokenFile = lib.mkOption {
      type = lib.types.path;
      description = "File containing the API auth token (shared by backend and frontend)";
    };

    tbEmailFile = lib.mkOption {
      type = lib.types.path;
      description = "File containing Total Battle login email";
    };

    tbPasswordFile = lib.mkOption {
      type = lib.types.path;
      description = "File containing Total Battle login password";
    };

    adminUserFile = lib.mkOption {
      type = lib.types.path;
      description = "File containing the admin panel username";
    };

    adminPasswordFile = lib.mkOption {
      type = lib.types.path;
      description = "File containing the admin panel password";
    };

    sessionSecretFile = lib.mkOption {
      type = lib.types.path;
      description = "File containing the session signing secret";
    };

    searchTarget = lib.mkOption {
      type = lib.types.str;
      default = "Mercenary Exchange";
      description = "Building name to search for (determines which reference image to use)";
    };

    navigateDelayMs = lib.mkOption {
      type = lib.types.int;
      default = 750;
      description = "Fly-animation wait after coordinate navigation, in milliseconds";
    };

    scanPattern = lib.mkOption {
      type = lib.types.str;
      default = "grid";
      description = "Scan pattern: single, multi, wide, grid, known";
    };

    scanRings = lib.mkOption {
      type = lib.types.nullOr lib.types.int;
      default = null;
      description = "Override ring count per scan pattern (null = use pattern default)";
    };

    knownLocationsFile = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Path to known locations CSV file (k,x,y format) for 'known' scan pattern";
    };

    exchangeLog = lib.mkOption {
      type = lib.types.str;
      default = "exchanges.jsonl";
      description = "Path to exchange detection JSONL log file";
    };

    chromiumPackage = lib.mkOption {
      type = lib.types.package;
      default = pkgs.chromium;
      defaultText = lib.literalExpression "pkgs.chromium";
      description = "Chromium package to use for headless browsing";
    };

    domain = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      description = "Domain for nginx virtual host (enables nginx when set)";
    };

    nginx.enableSSL = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Enable SSL via ACME for the nginx virtual host";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.mercy-backend = {
      description = "Mercy - Backend";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];

      environment = {
        MERCY_KINGDOMS = cfg.kingdoms;
        MERCY_LISTEN_ADDR = "127.0.0.1:${toString cfg.backendPort}";
        MERCY_CHROMIUM_PATH = "${cfg.chromiumPackage}/bin/chromium";
        MERCY_SEARCH_TARGET = cfg.searchTarget;
        MERCY_NAVIGATE_DELAY_MS = toString cfg.navigateDelayMs;
        MERCY_SCAN_PATTERN = cfg.scanPattern;
        MERCY_EXCHANGE_LOG = cfg.exchangeLog;
      } // lib.optionalAttrs (cfg.scanRings != null) {
        MERCY_SCAN_RINGS = toString cfg.scanRings;
      } // lib.optionalAttrs (cfg.knownLocationsFile != null) {
        MERCY_KNOWN_LOCATIONS = cfg.knownLocationsFile;
      };

      serviceConfig = {
        Type = "simple";
        DynamicUser = true;
        StateDirectory = "mercy";
        WorkingDirectory = "${cfg.backendPackage}/share/mercy";

        ExecStart = backendStartScript;

        Restart = "on-failure";
        RestartSec = 10;

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

    systemd.services.mercy-frontend = {
      description = "Mercy - Frontend";
      after = [ "mercy-backend.service" ];
      requires = [ "mercy-backend.service" ];
      wantedBy = [ "multi-user.target" ];

      environment = {
        PORT = toString cfg.frontendPort;
        HOSTNAME = "127.0.0.1";
        NODE_ENV = "production";
        MERCY_BACKEND_URL = "http://127.0.0.1:${toString cfg.backendPort}";
        NEXT_CACHE_DIR = "/var/cache/mercy-frontend";
      };

      serviceConfig = {
        Type = "simple";
        DynamicUser = true;
        CacheDirectory = "mercy-frontend";
        WorkingDirectory = "${cfg.frontendPackage}";

        ExecStartPre = "${pkgs.coreutils}/bin/rm -rf /var/cache/mercy-frontend/*";
        ExecStart = frontendStartScript;

        Restart = "on-failure";
        RestartSec = 5;

        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictNamespaces = true;
        RestrictSUIDSGID = true;
        MemoryDenyWriteExecute = false;
        ReadWritePaths = [ "/var/cache/mercy-frontend" ];
      };
    };

    services.nginx.virtualHosts = lib.mkIf (cfg.domain != null) {
      ${cfg.domain} = {
        forceSSL = cfg.nginx.enableSSL;
        enableACME = cfg.nginx.enableSSL;
        acmeRoot = null;
        locations."/" = {
          proxyPass = "http://127.0.0.1:${toString cfg.frontendPort}";
          proxyWebsockets = true;
        };
      };
    };
  };
}
