use crate::Severity;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum DangerousPatternPlatform {
    Unix,
    Windows,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct DangerousPatternSpec {
    pub id: &'static str,
    pub pattern: &'static str,
    pub severity: Severity,
    pub description: &'static str,
    pub platforms: &'static [DangerousPatternPlatform],
}

pub fn default_unix_dangerous_pattern_specs() -> &'static [DangerousPatternSpec] {
    UNIX_DANGEROUS_PATTERNS
}

pub fn default_windows_dangerous_pattern_specs() -> &'static [DangerousPatternSpec] {
    WINDOWS_DANGEROUS_PATTERNS
}

const UNIX: &[DangerousPatternPlatform] = &[DangerousPatternPlatform::Unix];
const WINDOWS: &[DangerousPatternPlatform] = &[DangerousPatternPlatform::Windows];

const UNIX_DANGEROUS_PATTERNS: &[DangerousPatternSpec] = &[
    spec(
        "unix-rm-rf-root",
        r"\brm\s+-[^|;&]*r[^|;&]*f[^|;&]*\s+(/|~)(\s|$)",
        Severity::Critical,
        "recursive force delete of root or home",
        UNIX,
    ),
    spec(
        "unix-sudo-rm-rf",
        r"\bsudo\s+rm\s+-[^|;&]*r[^|;&]*f[^|;&]*\s+/",
        Severity::Critical,
        "privileged recursive removal",
        UNIX,
    ),
    spec(
        "unix-chmod-777-root",
        r"\bchmod\s+-?R?\s+777\s+/(?:\s|$)",
        Severity::Critical,
        "world-writable root permissions",
        UNIX,
    ),
    spec(
        "unix-chown-recursive-root",
        r"\bchown\s+-R\s+[^ ]+\s+/(?:\s|$)",
        Severity::Critical,
        "recursive root ownership rewrite",
        UNIX,
    ),
    spec(
        "unix-curl-pipe-shell",
        r"\b(curl|wget)\b[^|;&]*\|\s*(sh|bash|zsh)\b",
        Severity::High,
        "downloaded script piped to shell",
        UNIX,
    ),
    spec(
        "unix-fork-bomb",
        r":\(\)\s*\{\s*:\|:\s*&\s*\}\s*;:",
        Severity::Critical,
        "shell fork bomb",
        UNIX,
    ),
    spec(
        "unix-shutdown",
        r"\b(shutdown|reboot|halt|poweroff)\b",
        Severity::Critical,
        "host shutdown command",
        UNIX,
    ),
    spec(
        "unix-dd-disk",
        r"\bdd\b[^|;&]*(of=/dev/(sd|disk|nvme|rdisk)|if=/dev/zero)",
        Severity::Critical,
        "raw disk overwrite",
        UNIX,
    ),
    spec(
        "unix-mkfs",
        r"\bmkfs(\.[a-z0-9]+)?\b",
        Severity::Critical,
        "filesystem creation over device",
        UNIX,
    ),
    spec(
        "unix-diskutil-erase",
        r"\bdiskutil\s+eraseDisk\b",
        Severity::Critical,
        "macOS disk erase",
        UNIX,
    ),
    spec(
        "unix-sudoers-write",
        r"\b(tee|cat|echo|visudo)\b[^|;&]*(/etc/sudoers|/etc/sudoers\.d/)",
        Severity::Critical,
        "sudoers mutation",
        UNIX,
    ),
    spec(
        "unix-passwd-shadow-write",
        r"\b(tee|cat|echo|sed)\b[^|;&]*(/etc/passwd|/etc/shadow)",
        Severity::Critical,
        "account database mutation",
        UNIX,
    ),
    spec(
        "unix-ssh-key-delete",
        r"\brm\s+-[^|;&]*f[^|;&]*\s+[^|;&]*(\.ssh/(id_|authorized_keys|known_hosts))",
        Severity::High,
        "ssh credential deletion",
        UNIX,
    ),
    spec(
        "git-force-push-main",
        r"\bgit\s+push\b[^|;&]*(--force|-f|--mirror)[^|;&]*(main|master)\b",
        Severity::High,
        "force push protected branch",
        UNIX,
    ),
    spec(
        "git-reset-hard-previous",
        r"\bgit\s+reset\s+--hard\s+HEAD~[0-9]*\b",
        Severity::High,
        "destructive git reset",
        UNIX,
    ),
    spec(
        "unix-authorized-keys-write",
        r"(>>|>)\s*(?:~|\$HOME)?/?.*\.ssh/authorized_keys\b",
        Severity::High,
        "ssh authorized_keys persistence",
        UNIX,
    ),
    spec(
        "unix-shell-profile-persistence",
        r"(>>|>)\s*(?:~|\$HOME)?/?\.(?:bashrc|zshrc|profile|bash_profile)\b",
        Severity::High,
        "shell profile persistence",
        UNIX,
    ),
    spec(
        "unix-read-aws-credentials",
        r"(?:cat|less|more|grep)\b.*(?:~|\$HOME)?/?\.aws/credentials\b",
        Severity::High,
        "cloud credential read",
        UNIX,
    ),
    spec(
        "unix-env-dump",
        r"(^|[;&|]\s*)env\s*>\s*/tmp/[^\s]+",
        Severity::Medium,
        "environment dump",
        UNIX,
    ),
    spec(
        "unix-clear-history",
        r"\bhistory\s+-c\b",
        Severity::Medium,
        "shell history clearing",
        UNIX,
    ),
    spec(
        "unix-kill-all",
        r"\b(killall|pkill)\s+(-9\s+)?(-u\s+\S+|.*)",
        Severity::High,
        "broad process termination",
        UNIX,
    ),
    spec(
        "unix-crontab-persistence",
        r"\b(crontab\s+|/etc/cron\.|/var/spool/cron)",
        Severity::High,
        "cron persistence mutation",
        UNIX,
    ),
    spec(
        "unix-launchctl-persistence",
        r"\blaunchctl\s+(load|bootstrap|enable)\b",
        Severity::High,
        "macOS launch agent persistence",
        UNIX,
    ),
];

const WINDOWS_DANGEROUS_PATTERNS: &[DangerousPatternSpec] = &[
    spec(
        "windows-remove-item-recurse-root",
        r"\bremove-item\b[^|;&]*(-recurse|-r)[^|;&]*(-force|-fo)[^|;&]*([a-z]:\\|c:\\)",
        Severity::Critical,
        "recursive force delete of drive root",
        WINDOWS,
    ),
    spec(
        "windows-format-volume",
        r"\b(format-volume|format\s+[a-z]:)\b",
        Severity::Critical,
        "format Windows volume",
        WINDOWS,
    ),
    spec(
        "windows-stop-computer",
        r"\b(stop-computer|restart-computer|shutdown\.exe|restart-computer)\b",
        Severity::Critical,
        "Windows shutdown or restart command",
        WINDOWS,
    ),
    spec(
        "windows-diskpart-clean",
        r"\bdiskpart\b[^|;&]*\bclean\b",
        Severity::Critical,
        "diskpart clean operation",
        WINDOWS,
    ),
    spec(
        "windows-iwr-iex",
        r"\b(iwr|irm|invoke-webrequest|invoke-restmethod)\b[^|;&]*\|\s*(iex|invoke-expression)\b",
        Severity::High,
        "downloaded script piped to PowerShell",
        WINDOWS,
    ),
    spec(
        "windows-webclient-iex",
        r"downloadstring\s*\([^)]*\)\s*\|\s*(iex|invoke-expression)\b",
        Severity::High,
        "WebClient downloaded script execution",
        WINDOWS,
    ),
    spec(
        "windows-cmd-del-root",
        r"\bdel\b[^|;&]*(/s|/q)[^|;&]*[a-z]:\\",
        Severity::Critical,
        "recursive cmd delete of drive",
        WINDOWS,
    ),
    spec(
        "windows-rmdir-root",
        r"\brmdir\b[^|;&]*(/s|/q)[^|;&]*[a-z]:\\",
        Severity::Critical,
        "recursive cmd rmdir of drive",
        WINDOWS,
    ),
    spec(
        "windows-executionpolicy-unrestricted",
        r"\bset-executionpolicy\b[^|;&]*(unrestricted|bypass)\b",
        Severity::High,
        "weaken PowerShell execution policy",
        WINDOWS,
    ),
    spec(
        "windows-disable-defender",
        r"\bset-mppreference\b[^|;&]*-disablerealtimemonitoring\b",
        Severity::Critical,
        "disable Defender realtime monitoring",
        WINDOWS,
    ),
    spec(
        "windows-add-trusted-publisher",
        r"\bcertutil\b[^|;&]*-addstore[^|;&]*(trustedpublisher|root)\b",
        Severity::High,
        "add trusted certificate publisher",
        WINDOWS,
    ),
    spec(
        "windows-registry-run-key",
        r"\b(reg\s+add|new-itemproperty)\b[^|;&]*(\\software\\microsoft\\windows\\currentversion\\run|currentversion\\run)",
        Severity::High,
        "Windows run key persistence",
        WINDOWS,
    ),
    spec(
        "windows-bcdedit-disable-recovery",
        r"\bbcdedit\b[^|;&]*(recoveryenabled\s+no|bootstatuspolicy\s+ignoreallfailures)",
        Severity::High,
        "disable boot recovery",
        WINDOWS,
    ),
    spec(
        "windows-vss-delete",
        r"\bvssadmin\b[^|;&]*delete\s+shadows\b",
        Severity::Critical,
        "delete shadow copies",
        WINDOWS,
    ),
    spec(
        "windows-wevtutil-clear",
        r"\bwevtutil\b[^|;&]*\bcl\b",
        Severity::High,
        "clear Windows event logs",
        WINDOWS,
    ),
];

const fn spec(
    id: &'static str,
    pattern: &'static str,
    severity: Severity,
    description: &'static str,
    platforms: &'static [DangerousPatternPlatform],
) -> DangerousPatternSpec {
    DangerousPatternSpec {
        id,
        pattern,
        severity,
        description,
        platforms,
    }
}
