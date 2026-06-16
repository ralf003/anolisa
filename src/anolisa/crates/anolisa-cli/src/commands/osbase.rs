use clap::{Args, Parser, Subcommand};

use anolisa_core::{
    OsbaseDomain, OsbaseInstallError, OsbaseInstallOutcome, OsbaseInstallRequest,
    OsbasePhaseStatus, RegisterHandler, execute_install,
};

use crate::context::CliContext;
use crate::response::{self, CliError};

#[derive(Parser)]
pub struct OsbaseArgs {
    #[command(subcommand)]
    pub command: OsbaseCommands,
}

#[derive(Subcommand)]
pub enum OsbaseCommands {
    /// Kernel variant management (agentic | stock)
    Kernel(KernelArgs),
    /// Sandbox scenario management
    /// (runc, rund, kata-fc, kata-clh, kata-qemu, firecracker, gvisor,
    /// gvisor-substrate, landlock)
    Sandbox(SandboxArgs),
    /// Security overlay management (loongshield, seccomp-profiles)
    Security(SecurityArgs),
}

// ===========================================================================
// Kernel
// ===========================================================================

#[derive(Parser)]
pub struct KernelArgs {
    #[command(subcommand)]
    pub command: KernelCommands,
}

#[derive(Subcommand)]
pub enum KernelCommands {
    /// Install a kernel variant
    Install(KernelInstallArgs),
    /// Remove kernel modules
    Remove,
    /// Show kernel substrate status
    Status,
}

#[derive(Args)]
pub struct KernelInstallArgs {
    /// Kernel variant: `agentic` or `stock`
    pub variant: String,

    /// Pin a specific kernel version
    #[arg(long)]
    pub version: Option<String>,

    /// Bootloader to configure
    #[arg(long, default_value = "auto")]
    pub bootloader: String,

    /// Set as next-boot default
    #[arg(long, default_value_t = false)]
    pub default: bool,
}

// ===========================================================================
// Sandbox
// ===========================================================================

#[derive(Parser)]
pub struct SandboxArgs {
    #[command(subcommand)]
    pub command: SandboxCommands,
}

#[derive(Subcommand)]
pub enum SandboxCommands {
    /// Install a sandbox scenario
    ///
    /// Runs the 5-phase install pipeline:
    /// Pre-flight → Packages → OS Primitives → Service → Verify
    Install(SandboxInstallArgs),

    /// Remove a sandbox scenario
    ///
    /// Runs the reverse 3-phase pipeline:
    /// Pre-check → Service Teardown → Cleanup
    Remove {
        /// Scenario to remove
        scenario: String,

        /// Also remove ANOLISA-written config files and data directories
        #[arg(long)]
        purge: bool,

        /// Skip dependency checks (dangerous: may break dependent scenarios)
        #[arg(long)]
        force: bool,

        /// Print removal plan without executing
        #[arg(long)]
        dry_run: bool,
    },

    /// List all sandbox scenarios and their availability
    ///
    /// Performs real-time environment probing (does not read cache).
    List {
        /// Only show scenarios whose gate conditions pass
        #[arg(long)]
        available: bool,

        /// Output as structured JSON
        #[arg(long)]
        json: bool,
    },

    /// Show sandbox scenario status
    ///
    /// Without scenario: summary of all scenarios. With scenario: detailed info.
    Status {
        /// Specific scenario to query (omit for all)
        scenario: Option<String>,

        /// Output as structured JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
pub struct SandboxInstallArgs {
    /// Sandbox scenario name (runc, rund, kata-fc, kata-clh, kata-qemu,
    /// firecracker, gvisor, gvisor-substrate, landlock)
    pub scenario: String,

    /// Register containerd runtime handler ("containerd" | "none")
    #[arg(long, default_value = "containerd")]
    pub register_handler: String,

    /// Additionally create a Kubernetes RuntimeClass resource
    #[arg(long, default_value_t = false)]
    pub register_runtimeclass: bool,

    /// Custom kata config.toml override path
    #[arg(long)]
    pub config: Option<String>,

    /// Set as default RuntimeClass after install
    #[arg(long, default_value_t = false)]
    pub default: bool,

    /// Skip non-fatal preflight warnings
    #[arg(long, default_value_t = false)]
    pub force: bool,

    /// Skip post-install smoke check
    #[arg(long, default_value_t = false)]
    pub no_verify: bool,
}

// ===========================================================================
// Security
// ===========================================================================

#[derive(Parser)]
pub struct SecurityArgs {
    #[command(subcommand)]
    pub command: SecurityCommands,
}

#[derive(Subcommand)]
pub enum SecurityCommands {
    /// Install a security overlay
    Install {
        /// Target: loongshield, seccomp-profiles
        target: String,
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove a security overlay
    Remove { target: String },
    /// Show security overlay status
    Status { target: Option<String> },
}

// ===========================================================================
// Dispatch
// ===========================================================================

pub fn handle(args: OsbaseArgs, ctx: &CliContext) -> Result<(), CliError> {
    // osbase manipulates `/boot`, `/etc`, kernel modules and systemd
    // units — every operation is implicitly system-mode. Rather than
    // letting the request fail deep inside a 5-phase pipeline with a
    // permission-denied IO error, gate it at the top with two
    // pre-checks. The order is deliberate: reject `--install-mode=user`
    // first (it's a stale flag from the user's command line, fixable
    // without re-running with sudo) before we tell them to escalate.
    osbase_preflight(ctx)?;

    match args.command {
        OsbaseCommands::Sandbox(s) => handle_sandbox(s.command, ctx),
        OsbaseCommands::Kernel(k) => handle_kernel(k.command, ctx),
        OsbaseCommands::Security(s) => handle_security(s.command),
    }
}

/// Top-level guard for `anolisa osbase`: enforce system mode and root.
///
/// Both checks live in this dedicated helper so they can be unit-tested
/// (the privilege probe is platform-agnostic via
/// `anolisa_platform::privilege::is_root`) and so per-domain handlers
/// don't need to repeat the boilerplate. `anolisa-core::validate_request`
/// repeats the uid check defensively for callers that bypass the CLI
/// layer (library use, tests).
fn osbase_preflight(ctx: &CliContext) -> Result<(), CliError> {
    if !matches!(ctx.install_mode, crate::context::InstallMode::System) {
        return Err(CliError::PermissionDenied {
            command: "osbase".to_string(),
            reason: "osbase does not support --install-mode=user; \
                 osbase only operates in system mode"
                .to_string(),
            hint: Some(
                "drop --install-mode=user (or pass --install-mode=system) \
                 and re-run with: sudo anolisa osbase ..."
                    .to_string(),
            ),
        });
    }

    if !anolisa_platform::privilege::is_root() {
        return Err(CliError::PermissionDenied {
            command: "osbase".to_string(),
            reason: "osbase requires root privileges (writes to /boot, \
                 /etc, kernel modules, systemd units)"
                .to_string(),
            hint: Some("re-run with: sudo anolisa osbase ...".to_string()),
        });
    }

    Ok(())
}

fn handle_kernel(command: KernelCommands, _ctx: &CliContext) -> Result<(), CliError> {
    // Kernel domain is a stub in `osbase_install::execute_install` (Task #4);
    // surface a stable not-implemented envelope until the dedicated pipeline
    // lands. The clap layer has already validated the variant/version
    // shape, so error context here can stay coarse.
    let cmd = match command {
        KernelCommands::Install(args) => format!("osbase kernel install {}", args.variant),
        KernelCommands::Remove => "osbase kernel remove".to_string(),
        KernelCommands::Status => "osbase kernel status".to_string(),
    };
    Err(CliError::not_implemented(cmd))
}

fn handle_security(command: SecurityCommands) -> Result<(), CliError> {
    let cmd = match command {
        SecurityCommands::Install { target, .. } => format!("osbase security install {target}"),
        SecurityCommands::Remove { target } => format!("osbase security remove {target}"),
        SecurityCommands::Status { target } => match target {
            Some(t) => format!("osbase security status {t}"),
            None => "osbase security status".to_string(),
        },
    };
    Err(CliError::not_implemented(cmd))
}

fn handle_sandbox(command: SandboxCommands, ctx: &CliContext) -> Result<(), CliError> {
    match command {
        SandboxCommands::Install(args) => handle_sandbox_install(args, ctx),
        SandboxCommands::Remove { scenario, .. } => Err(CliError::not_implemented(format!(
            "osbase sandbox remove {scenario}"
        ))),
        SandboxCommands::List { .. } => Err(CliError::not_implemented("osbase sandbox list")),
        SandboxCommands::Status { scenario, .. } => {
            let cmd = match scenario {
                Some(s) => format!("osbase sandbox status {s}"),
                None => "osbase sandbox status".to_string(),
            };
            Err(CliError::not_implemented(cmd))
        }
    }
}

fn handle_sandbox_install(args: SandboxInstallArgs, ctx: &CliContext) -> Result<(), CliError> {
    let cmd = format!("osbase sandbox install {}", args.scenario);

    // System-mode + root were already enforced by `osbase_preflight` at
    // the top-level dispatcher; no need to re-check here. Keeping the
    // guard at exactly one place avoids drift between osbase verbs
    // (kernel install / security install / sandbox install would all
    // need the same policy otherwise).

    let register_handler = match args.register_handler.as_str() {
        "containerd" => RegisterHandler::Containerd,
        "none" => RegisterHandler::None,
        other => {
            return Err(CliError::InvalidArgument {
                command: cmd,
                reason: format!(
                    "--register-handler must be 'containerd' or 'none' (got '{other}')"
                ),
            });
        }
    };

    let request = OsbaseInstallRequest {
        domain: OsbaseDomain::Sandbox,
        target: args.scenario.clone(),
        register_handler,
        register_runtimeclass: args.register_runtimeclass,
        config_override: args.config.clone(),
        set_default: args.default,
        force: args.force,
        skip_verify: args.no_verify,
        dry_run: ctx.dry_run,
    };

    let env = anolisa_env::EnvService::detect();

    match execute_install(&request, &env) {
        Ok(outcome) => render_install_outcome(ctx, &outcome),
        Err(err) => Err(map_osbase_err(err, &cmd)),
    }
}

// ===========================================================================
// Outcome / error rendering
// ===========================================================================

fn render_install_outcome(
    ctx: &CliContext,
    outcome: &OsbaseInstallOutcome,
) -> Result<(), CliError> {
    let cmd = format!(
        "osbase {} install {}",
        outcome.domain.as_str(),
        outcome.target
    );

    // For non-zero outcomes (degraded / failed) the JSON envelope must
    // carry ok=false so machine callers don't see a success envelope
    // contradicting the non-zero exit code. Build the CliError up front
    // and let `render_error` (called by main on Err) emit the error
    // envelope on the JSON path.
    let outcome_err: Option<CliError> = match outcome.exit_code {
        0 => None,
        2 => Some(CliError::Degraded {
            command: cmd.clone(),
            reason: format!(
                "{} scenario '{}' installed with warnings",
                outcome.domain.as_str(),
                outcome.target
            ),
        }),
        _ => Some(CliError::Runtime {
            command: cmd.clone(),
            reason: format!(
                "{} scenario '{}' install failed (exit_code={})",
                outcome.domain.as_str(),
                outcome.target,
                outcome.exit_code
            ),
        }),
    };

    if ctx.json {
        if let Some(err) = outcome_err {
            return Err(err);
        }
        return response::render_json(&cmd, outcome_to_json(outcome));
    }

    for (i, phase) in outcome.phases.iter().enumerate() {
        let icon = match phase.status {
            OsbasePhaseStatus::Success => "\u{2713}",
            OsbasePhaseStatus::Skipped => "\u{2298}",
            OsbasePhaseStatus::Degraded => "\u{26A0}",
            OsbasePhaseStatus::Failed => "\u{2717}",
        };
        let phase_name = format!("{:<14}", phase.name);
        let msg = phase.message.as_deref().unwrap_or("");
        println!(
            "[{}/{}] {} {}  ({})",
            i + 1,
            outcome.phases.len(),
            phase_name,
            icon,
            msg
        );
    }
    println!();

    if outcome.exit_code == 0 {
        println!(
            "{} scenario '{}' installed successfully.",
            outcome.domain.as_str(),
            outcome.target
        );
    } else if outcome.exit_code == 2 {
        println!(
            "{} scenario '{}' installed with warnings (degraded).",
            outcome.domain.as_str(),
            outcome.target
        );
    }

    if !outcome.warnings.is_empty() {
        eprintln!();
        for w in &outcome.warnings {
            eprintln!("warning: {w}");
        }
    }

    // Surface non-zero exit_code to the process exit. Without this, an
    // Ok(outcome) carrying exit_code=2/3 would mask degraded installs
    // from CI / scripts that only inspect $?.
    match outcome_err {
        None => Ok(()),
        Some(err) => Err(err),
    }
}

fn outcome_to_json(outcome: &OsbaseInstallOutcome) -> serde_json::Value {
    let phases: Vec<serde_json::Value> = outcome
        .phases
        .iter()
        .map(|p| {
            serde_json::json!({
                "name": p.name,
                "status": match p.status {
                    OsbasePhaseStatus::Success => "success",
                    OsbasePhaseStatus::Skipped => "skipped",
                    OsbasePhaseStatus::Degraded => "degraded",
                    OsbasePhaseStatus::Failed => "failed",
                },
                "message": p.message,
                "duration_ms": p.duration_ms,
            })
        })
        .collect();
    serde_json::json!({
        "domain": outcome.domain.as_str(),
        "target": outcome.target,
        "exit_code": outcome.exit_code,
        "phases": phases,
        "warnings": outcome.warnings,
    })
}

fn map_osbase_err(err: OsbaseInstallError, command: &str) -> CliError {
    match &err {
        OsbaseInstallError::Unsupported(_) | OsbaseInstallError::InvalidRequest { .. } => {
            CliError::InvalidArgument {
                command: command.to_string(),
                reason: err.to_string(),
            }
        }
        OsbaseInstallError::PhaseFailed { .. } | OsbaseInstallError::Io(_) => CliError::Runtime {
            command: command.to_string(),
            reason: err.to_string(),
        },
    }
}
