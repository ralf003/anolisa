//! Generic osbase install entry layer.
//!
//! Provides a domain-agnostic surface (`Kernel` / `Sandbox` / `Security`) on
//! top of the existing per-domain pipelines. The `Sandbox` domain bridges
//! into the mature [`crate::sandbox_install`] 5-phase orchestrator without
//! modifying it; `Kernel` and `Security` are stubs pending dedicated
//! pipelines.
//!
//! Design references:
//! - osbase install task pool (Task #4): generalized entry layer
//! - manifest v2 `[[support_matrix]]` / `[scenario_defaults]` (Task #6)
//!
//! The CLI handler should migrate to [`execute_install`] over time; the
//! existing `sandbox_install::execute_sandbox_install` API is retained as a
//! deprecated re-export from [`crate`] to give callers a transition window.

use anolisa_env::EnvFacts;
use anolisa_platform::fs_layout::FsLayout;

use crate::sandbox_install::{
    self, SandboxBackendKind, SandboxInstallError, SandboxInstallOutcome, SandboxInstallRequest,
};
use crate::support_matrix::UnsupportedError;

// ===========================================================================
// Public types
// ===========================================================================

/// The three osbase domains. Each domain owns a distinct install pipeline;
/// dispatch happens in [`execute_install`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsbaseDomain {
    /// Linux kernel variants (e.g. `agentic`, `vanilla`).
    Kernel,
    /// Sandbox engines (runc / rund / kata-* / gvisor / firecracker / landlock).
    Sandbox,
    /// Security primitives (LSMs, audit, seccomp profiles).
    Security,
}

impl OsbaseDomain {
    /// Stable lower-case identifier used in logs and error strings.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kernel => "kernel",
            Self::Sandbox => "sandbox",
            Self::Security => "security",
        }
    }
}

/// Whether to register the engine into a containerd handler entry.
///
/// Mirrors `sandbox_install::SandboxInstallRequest::runtime` in a typed form
/// so callers don't pass stringly-typed runtime names through the entry
/// layer. Only `Containerd` is currently meaningful; `None` means standalone
/// (no L2 wiring).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegisterHandler {
    /// Register with containerd via the appropriate shim.
    #[default]
    Containerd,
    /// Standalone install — no L2 runtime wiring.
    None,
}

/// Generic install request for any osbase domain.
///
/// The CLI translates `anolisa install <domain> <target> [flags]` into this
/// struct. `target` carries the scenario name (Sandbox: `runc`, `rund`,
/// `kata-clh`, `gvisor`, ...) or the kernel variant (`agentic`, `vanilla`).
#[derive(Debug, Clone)]
pub struct OsbaseInstallRequest {
    /// Which domain pipeline to dispatch to.
    pub domain: OsbaseDomain,
    /// Scenario name (Sandbox) or variant (Kernel/Security). Must be
    /// non-empty; matched against the manifest set in the dispatch step.
    pub target: String,
    /// L2 handler registration mode. Ignored for `Kernel` and `Security`
    /// (a warning is surfaced if non-default).
    pub register_handler: RegisterHandler,
    /// Additionally create a Kubernetes `RuntimeClass` after handler
    /// registration. Requires `register_handler != None`.
    pub register_runtimeclass: bool,
    /// Optional `--config` override path passed through to the domain
    /// pipeline. Currently informational — ignored by the sandbox bridge.
    pub config_override: Option<String>,
    /// Mark the installed engine as the default runtime for its handler.
    pub set_default: bool,
    /// Bypass non-fatal pre-flight gates.
    pub force: bool,
    /// Skip the post-install verify phase.
    pub skip_verify: bool,
    /// Produce a plan without side effects.
    pub dry_run: bool,
}

/// Aggregate outcome of a generic install.
#[derive(Debug, Clone)]
pub struct OsbaseInstallOutcome {
    pub domain: OsbaseDomain,
    pub target: String,
    pub phases: Vec<PhaseResult>,
    /// `0` success, `1` failed, `2` degraded. Mirrors the sandbox-subsystem
    /// exit-code table after collapsing the `Skipped`/`Warning` distinction.
    pub exit_code: i32,
    pub warnings: Vec<String>,
}

/// Per-phase result in domain-agnostic shape.
///
/// Phase names align with the sandbox 5-phase pipeline so JSON consumers can
/// keep a single schema across domains:
/// `preflight` / `packages` / `os_primitives` / `service_setup` / `post_verify`.
#[derive(Debug, Clone)]
pub struct PhaseResult {
    pub name: String,
    pub status: PhaseStatus,
    pub message: Option<String>,
    pub duration_ms: Option<u64>,
}

/// Status of a single phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaseStatus {
    Success,
    Skipped,
    Degraded,
    Failed,
}

/// Errors surfaced by the generic install entry.
#[derive(Debug, thiserror::Error)]
pub enum OsbaseInstallError {
    /// The `(env, target)` pair did not match any `[[support_matrix]]` row.
    #[error("unsupported: {0}")]
    Unsupported(#[from] UnsupportedError),

    /// Request shape is invalid before any dispatch happens.
    #[error("invalid request: {reason}")]
    InvalidRequest { reason: String },

    /// A pipeline phase failed; the inner pipeline is responsible for
    /// best-effort rollback before this is surfaced.
    #[error("phase '{phase}' failed: {message}")]
    PhaseFailed { phase: String, message: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

// ===========================================================================
// Entry point
// ===========================================================================

/// Validate the request and dispatch to the appropriate domain pipeline.
///
/// `env` carries the host facts (`os_id`, `os_version`, `arch`, `kernel`)
/// consumed by support-matrix matching inside each domain pipeline.
///
/// Currently only the `Sandbox` domain is wired; `Kernel` and `Security`
/// return [`OsbaseInstallError::InvalidRequest`] until their pipelines land.
pub fn execute_install(
    request: &OsbaseInstallRequest,
    env: &EnvFacts,
) -> Result<OsbaseInstallOutcome, OsbaseInstallError> {
    validate_request(request, env)?;

    match request.domain {
        OsbaseDomain::Sandbox => sandbox_dispatch(request, env),
        OsbaseDomain::Kernel => Err(OsbaseInstallError::InvalidRequest {
            reason: "kernel install not yet implemented".to_string(),
        }),
        OsbaseDomain::Security => Err(OsbaseInstallError::InvalidRequest {
            reason: "security install not yet implemented".to_string(),
        }),
    }
}

/// Lightweight request validation — runs before any side effect.
///
/// Hard rejections:
/// - empty `target`
/// - `register_runtimeclass=true` with `register_handler=None` (no handler
///   to attach the RuntimeClass to)
/// - `env.uid != 0` (osbase is implicitly system-mode; defensive check
///   that mirrors the CLI-layer guard for callers that bypass
///   `anolisa-cli` — library users, integration tests, future REST
///   front-ends)
///
/// Soft warnings (returned via the `Ok` outcome would require a logger; we
/// instead surface them through the dispatch layer on a successful run).
pub fn validate_request(
    request: &OsbaseInstallRequest,
    env: &EnvFacts,
) -> Result<(), OsbaseInstallError> {
    if request.target.trim().is_empty() {
        return Err(OsbaseInstallError::InvalidRequest {
            reason: "target must not be empty".to_string(),
        });
    }

    if request.register_runtimeclass && request.register_handler == RegisterHandler::None {
        return Err(OsbaseInstallError::InvalidRequest {
            reason: "--register-runtimeclass requires a non-None --register-handler".to_string(),
        });
    }

    // Defense-in-depth: the CLI dispatcher (`commands/osbase.rs`) already
    // rejects non-root invocations with a `PermissionDenied` envelope.
    // Repeat the check here so library callers — tests, future REST
    // front-ends, anything that calls `execute_install` directly —
    // can't slip past the privilege gate. Surfaced as `InvalidRequest`
    // (not a new variant) to keep the error model stable; the message
    // points at `sudo` so non-CLI callers still get an actionable hint.
    if env.uid != 0 {
        return Err(OsbaseInstallError::InvalidRequest {
            reason: "osbase requires root (uid=0); re-run with sudo".to_string(),
        });
    }

    Ok(())
}

// ===========================================================================
// Sandbox bridge
// ===========================================================================

/// Mapping from a sandbox scenario name to the legacy
/// `(SandboxBackendKind, variant, runtime, control_panel)` tuple consumed by
/// `sandbox_install::execute_sandbox_install`.
struct SandboxScenario {
    backend: SandboxBackendKind,
    variant: &'static str,
    /// `Some(rt)` defaults the runtime if the caller asked for handler
    /// registration; `None` means the engine has no L2 wiring at all
    /// (firecracker / landlock).
    runtime: Option<&'static str>,
    control_panel: Option<&'static str>,
}

/// Resolve a scenario name (`runc`, `rund`, `kata-clh`, `gvisor`, ...) to
/// the existing sandbox backend tuple. Returns `None` for unknown scenarios
/// — the caller turns this into [`OsbaseInstallError::InvalidRequest`].
fn resolve_sandbox_scenario(target: &str) -> Option<SandboxScenario> {
    let (backend, variant, runtime, control_panel) = match target {
        "runc" => (
            SandboxBackendKind::Container,
            "runc",
            Some("containerd"),
            None,
        ),
        "rund" => (SandboxBackendKind::Kata, "rund", Some("containerd"), None),
        "kata-qemu" => (SandboxBackendKind::Kata, "qemu", Some("containerd"), None),
        "kata-clh" => (SandboxBackendKind::Kata, "clh", Some("containerd"), None),
        "kata-fc" => (SandboxBackendKind::Firecracker, "kata-fc", None, None),
        "firecracker" => (SandboxBackendKind::Firecracker, "standard", None, None),
        "gvisor" => (
            SandboxBackendKind::Gvisor,
            "default",
            Some("containerd"),
            None,
        ),
        "gvisor-substrate" => (
            SandboxBackendKind::Gvisor,
            "default",
            Some("containerd"),
            Some("substrate"),
        ),
        "landlock" => (SandboxBackendKind::Landlock, "default", None, None),
        _ => return None,
    };
    Some(SandboxScenario {
        backend,
        variant,
        runtime,
        control_panel,
    })
}

/// Translate the generic request into a [`SandboxInstallRequest`] and call
/// the legacy 5-phase pipeline. The [`FsLayout`] is taken as the
/// system-default (`FsLayout::system(None)`); the CLI front-end will
/// eventually pass its own layout through, at which point this helper grows
/// a `layout` parameter — keep the bridge thin until then.
fn sandbox_dispatch(
    request: &OsbaseInstallRequest,
    _env: &EnvFacts,
) -> Result<OsbaseInstallOutcome, OsbaseInstallError> {
    let scenario = resolve_sandbox_scenario(&request.target).ok_or_else(|| {
        OsbaseInstallError::InvalidRequest {
            reason: format!("unknown sandbox scenario '{}'", request.target),
        }
    })?;

    // Map register_handler → runtime. `Containerd` keeps the scenario's
    // default runtime (None for engines that bypass L2); `None` forces
    // standalone even when the scenario has a default.
    let runtime = match request.register_handler {
        RegisterHandler::Containerd => scenario.runtime.map(str::to_string),
        RegisterHandler::None => None,
    };

    let mut warnings: Vec<String> = Vec::new();
    if request.config_override.is_some() {
        warnings.push("--config override is not yet honored by the sandbox bridge".to_string());
    }
    if request.set_default {
        warnings.push("--default is not yet honored by the sandbox bridge".to_string());
    }
    if request.register_runtimeclass {
        warnings
            .push("--register-runtimeclass is not yet honored by the sandbox bridge".to_string());
    }

    let sandbox_req = SandboxInstallRequest {
        backend: scenario.backend,
        variant: scenario.variant.to_string(),
        runtime,
        control_panel: scenario.control_panel.map(str::to_string),
        dry_run: request.dry_run,
        force: request.force,
        no_verify: request.skip_verify,
        json: false,
    };

    // TODO(osbase-install): plumb FsLayout through `execute_install` once
    // the CLI layer is migrated. Defaulting to system layout matches the
    // pre-existing `anolisa install` behaviour.
    let layout = FsLayout::system(None);

    match sandbox_install::execute_sandbox_install(&sandbox_req, &layout) {
        Ok(outcome) => Ok(map_sandbox_outcome(request, outcome, warnings)),
        Err(err) => Err(map_sandbox_error(request, err)),
    }
}

/// Translate the legacy [`SandboxInstallOutcome`] into the generic shape.
fn map_sandbox_outcome(
    request: &OsbaseInstallRequest,
    outcome: SandboxInstallOutcome,
    mut warnings: Vec<String>,
) -> OsbaseInstallOutcome {
    warnings.extend(outcome.warnings);

    let phases = outcome
        .phases
        .into_iter()
        .map(|p| PhaseResult {
            name: phase_to_name(p.phase).to_string(),
            status: map_phase_status(p.status),
            message: if p.message.is_empty() {
                None
            } else {
                Some(p.message)
            },
            duration_ms: None,
        })
        .collect();

    let exit_code = match outcome.exit_code {
        0 => 0,
        2 => 2, // degraded
        _ => 1, // failed (1 / 3 / 4 from the legacy table)
    };

    OsbaseInstallOutcome {
        domain: request.domain,
        target: request.target.clone(),
        phases,
        exit_code,
        warnings,
    }
}

/// Translate a legacy [`SandboxInstallError`] into the generic error type.
fn map_sandbox_error(
    request: &OsbaseInstallRequest,
    err: SandboxInstallError,
) -> OsbaseInstallError {
    match err {
        SandboxInstallError::Unsupported { backend, variant } => {
            OsbaseInstallError::InvalidRequest {
                reason: format!(
                    "scenario '{}' rejected by sandbox pipeline: {} {}",
                    request.target, backend, variant
                ),
            }
        }
        SandboxInstallError::EnvNotSatisfied {
            reason,
            remediation,
        } => {
            let mut msg = reason;
            if let Some(r) = remediation {
                msg.push_str(" — ");
                msg.push_str(&r);
            }
            OsbaseInstallError::PhaseFailed {
                phase: "preflight".to_string(),
                message: msg,
            }
        }
        SandboxInstallError::PackageFailed(m) => OsbaseInstallError::PhaseFailed {
            phase: "packages".to_string(),
            message: m,
        },
        SandboxInstallError::OsConfigFailed(m) => OsbaseInstallError::PhaseFailed {
            phase: "os_primitives".to_string(),
            message: m,
        },
        SandboxInstallError::ServiceFailed(m) => OsbaseInstallError::PhaseFailed {
            phase: "service_setup".to_string(),
            message: m,
        },
        SandboxInstallError::VerifyFailed(m) => OsbaseInstallError::PhaseFailed {
            phase: "post_verify".to_string(),
            message: m,
        },
        SandboxInstallError::LockHeld => OsbaseInstallError::InvalidRequest {
            reason: "install lock held by another process".to_string(),
        },
        SandboxInstallError::StateFailed(m) => OsbaseInstallError::PhaseFailed {
            phase: "state".to_string(),
            message: m,
        },
        SandboxInstallError::NotRoot => OsbaseInstallError::InvalidRequest {
            reason: "must run as root for system-mode install".to_string(),
        },
    }
}

fn phase_to_name(phase: sandbox_install::InstallPhase) -> &'static str {
    use sandbox_install::InstallPhase;
    match phase {
        InstallPhase::Preflight => "preflight",
        InstallPhase::Packages => "packages",
        InstallPhase::OsPrimitives => "os_primitives",
        InstallPhase::ServiceSetup => "service_setup",
        InstallPhase::PostVerify => "post_verify",
    }
}

fn map_phase_status(s: sandbox_install::PhaseStatus) -> PhaseStatus {
    use sandbox_install::PhaseStatus as Legacy;
    match s {
        Legacy::Success => PhaseStatus::Success,
        Legacy::Skipped => PhaseStatus::Skipped,
        Legacy::Warning => PhaseStatus::Degraded,
        Legacy::Failed => PhaseStatus::Failed,
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn req(domain: OsbaseDomain, target: &str) -> OsbaseInstallRequest {
        OsbaseInstallRequest {
            domain,
            target: target.to_string(),
            register_handler: RegisterHandler::Containerd,
            register_runtimeclass: false,
            config_override: None,
            set_default: false,
            force: false,
            skip_verify: false,
            dry_run: true,
        }
    }

    #[test]
    fn validate_rejects_empty_target() {
        let r = req(OsbaseDomain::Sandbox, "  ");
        assert!(matches!(
            validate_request(&r, &root_env()),
            Err(OsbaseInstallError::InvalidRequest { .. })
        ));
    }

    #[test]
    fn validate_rejects_runtimeclass_without_handler() {
        let mut r = req(OsbaseDomain::Sandbox, "runc");
        r.register_handler = RegisterHandler::None;
        r.register_runtimeclass = true;
        assert!(matches!(
            validate_request(&r, &root_env()),
            Err(OsbaseInstallError::InvalidRequest { .. })
        ));
    }

    #[test]
    fn validate_accepts_minimal_request() {
        assert!(validate_request(&req(OsbaseDomain::Sandbox, "runc"), &root_env()).is_ok());
    }

    #[test]
    fn validate_rejects_non_root_uid() {
        // Defensive uid check belongs in validate_request: a library
        // caller that builds a request manually (no CLI guard in front)
        // must still be told to escalate before any pipeline phase
        // touches /etc or /boot.
        let r = req(OsbaseDomain::Sandbox, "runc");
        let env = test_env(); // uid=1000
        match validate_request(&r, &env) {
            Err(OsbaseInstallError::InvalidRequest { reason }) => {
                assert!(
                    reason.contains("sudo"),
                    "expected hint pointing at sudo, got: {reason}"
                );
            }
            other => panic!("expected InvalidRequest for non-root uid, got {other:?}"),
        }
    }

    #[test]
    fn kernel_domain_is_stub() {
        let r = req(OsbaseDomain::Kernel, "agentic");
        let env = root_env();
        let err = execute_install(&r, &env).expect_err("kernel stub");
        assert!(matches!(err, OsbaseInstallError::InvalidRequest { .. }));
    }

    #[test]
    fn security_domain_is_stub() {
        let r = req(OsbaseDomain::Security, "selinux");
        let env = root_env();
        let err = execute_install(&r, &env).expect_err("security stub");
        assert!(matches!(err, OsbaseInstallError::InvalidRequest { .. }));
    }

    #[test]
    fn unknown_sandbox_scenario_is_invalid_request() {
        let r = req(OsbaseDomain::Sandbox, "nope-not-a-scenario");
        let env = root_env();
        let err = execute_install(&r, &env).expect_err("unknown scenario");
        match err {
            OsbaseInstallError::InvalidRequest { reason } => {
                assert!(reason.contains("nope-not-a-scenario"));
            }
            other => panic!("expected InvalidRequest, got {other:?}"),
        }
    }

    #[test]
    fn known_scenarios_resolve() {
        for s in [
            "runc",
            "rund",
            "kata-qemu",
            "kata-clh",
            "kata-fc",
            "firecracker",
            "gvisor",
            "gvisor-substrate",
            "landlock",
        ] {
            assert!(
                resolve_sandbox_scenario(s).is_some(),
                "scenario '{s}' should resolve"
            );
        }
    }

    fn test_env() -> EnvFacts {
        EnvFacts {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
            libc: None,
            kernel: Some("6.6.30".to_string()),
            pkg_base: None,
            os_id: Some("alinux".to_string()),
            os_version: Some("4".to_string()),
            btf: None,
            cap_bpf: None,
            container: None,
            user: "tester".to_string(),
            uid: 1000,
            home: std::path::PathBuf::from("/home/tester"),
        }
    }

    /// Same as `test_env` but with `uid=0`, used by tests that exercise
    /// downstream logic past the root-uid gate. Keeps the privilege
    /// check itself testable through `test_env` while letting other
    /// tests focus on their actual subject.
    fn root_env() -> EnvFacts {
        EnvFacts {
            uid: 0,
            user: "root".to_string(),
            ..test_env()
        }
    }
}
