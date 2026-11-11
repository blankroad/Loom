//! `RegistryProbe`: run a domain's `register` and check what a gateway will rely on.

use std::fmt;

use loom_domain_api::{DOMAIN_API_VERSION, DomainModule, Registry};

/// Summary of a registration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProbeReport {
    /// Domain.
    pub domain: String,
    /// Command methods.
    pub commands: Vec<String>,
    /// Write command methods.
    pub write_commands: Vec<String>,
    /// Event kinds.
    pub event_kinds: Vec<String>,
    /// Worker kinds.
    pub workers: Vec<String>,
    /// Problems that make the domain unusable.
    pub errors: Vec<String>,
    /// Things to look at.
    pub warnings: Vec<String>,
}

impl fmt::Display for ProbeReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "domain {:?}: {} commands ({} write), {} event kinds, workers {:?}",
            self.domain,
            self.commands.len(),
            self.write_commands.len(),
            self.event_kinds.len(),
            self.workers
        )?;
        for e in &self.errors {
            writeln!(f, "  ERROR: {e}")?;
        }
        for w in &self.warnings {
            writeln!(f, "  warning: {w}")?;
        }
        Ok(())
    }
}

/// Registration checker.
#[derive(Debug, Default)]
pub struct RegistryProbe;

impl RegistryProbe {
    /// Register `module` into a fresh registry and check it. Returns the registry for further
    /// inspection plus the report (`Err` when `register` itself failed).
    pub fn run<M: DomainModule>(module: &M) -> Result<(Registry, ProbeReport), String> {
        let mut reg = Registry::new(M::NAME).map_err(|e| format!("Registry::new({:?}): {e}", M::NAME))?;
        module
            .register(&mut reg)
            .map_err(|e| format!("{}::register failed: {e}", M::NAME))?;
        let mut r = ProbeReport {
            domain: M::NAME.into(),
            ..Default::default()
        };
        if M::API_VERSION != DOMAIN_API_VERSION {
            r.errors
                .push(format!("API_VERSION {} != {DOMAIN_API_VERSION}", M::API_VERSION));
        }
        for c in reg.commands() {
            r.commands.push(c.method.clone());
            if c.write_class.is_some() {
                r.write_commands.push(c.method.clone());
            }
        }
        r.event_kinds = reg.event_kinds().iter().map(|k| k.kind.clone()).collect();
        r.workers = reg.workers().iter().map(|w| w.kind.clone()).collect();
        if r.commands.is_empty() {
            r.errors.push("no commands registered".into());
        }
        if reg.help_ref().is_none() {
            r.errors
                .push("no help (C8: `loom <domain> help --json` is generated from it)".into());
        }
        if !r.write_commands.is_empty() {
            if reg.hooks_ref().is_none() {
                r.errors.push("write commands but no WriteHooks (A2)".into());
            }
            match reg.risk_table_ref() {
                None => r.errors.push("write commands but no risk table (D6)".into()),
                Some(t) => {
                    for m in &r.write_commands {
                        if !t.rows.iter().any(|row| &row.method == m) {
                            r.warnings.push(format!("{m}: no risk table row"));
                        }
                    }
                }
            }
        }
        if r.event_kinds.is_empty() {
            r.warnings
                .push("no event kinds registered — nothing will show on the user's screen".into());
        }
        for k in reg.event_kinds() {
            if k.pointers.is_empty() {
                r.warnings
                    .push(format!("{}: no fixed JSON Pointers for evidence", k.kind));
            }
        }
        Ok((reg, r))
    }

    /// Like [`RegistryProbe::run`] but panics with a readable report on any error.
    pub fn assert_ok<M: DomainModule>(module: &M) -> (Registry, ProbeReport) {
        match Self::run(module) {
            Ok((reg, r)) if r.errors.is_empty() => (reg, r),
            Ok((_, r)) => panic!("registry probe failed:\n{r}"),
            Err(e) => panic!("registry probe failed: {e}"),
        }
    }
}
