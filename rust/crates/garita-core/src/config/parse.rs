use std::collections::BTreeMap;
use std::time::Duration;

use yaml_serde::{Mapping, Value};

use super::{
    Budget, Budgets, Builtin, CONFIG_SCHEMA_VERSION, Check, CheckForm, Config, ConfigError,
    ConfigErrors, DEFAULT_PARSER, DEFAULT_RECEIPTS_KEEP, DEFAULT_TIMEOUT_FACTOR, DurationError,
    EvidenceAccept, MergeConfig, MergeMode, OnCrash, OnExceed, Partition, Regression,
    TriggerConfig, VersionReq, Worktree, parse_duration,
};
use crate::argv::{Arg, Command, Placeholder};
use crate::report::{CheckKind, ScopeKind, Severity, Trigger};

const TOP_KEYS: &[&str] = &[
    "schema_version",
    "requires",
    "triggers",
    "budgets",
    "on_crash",
    "timeout",
    "receipts",
    "merge",
    "evidence",
    "checks",
];
const CHECK_KEYS: &[&str] = &[
    "run",
    "parser",
    "native",
    "builtin",
    "globs",
    "exclude",
    "partition",
    "via",
    "cwd",
    "kind",
    "budget",
    "on_exceed",
    "on",
    "advisory",
    "min_severity",
];
const BUILTIN_FORBIDS: &[&str] = &["parser", "globs", "partition", "via", "exclude", "cwd"];
pub(super) const RESERVED_CHECK: &str = "garita";
pub(super) const VIA_TURBO: &str = "turbo";
const MIN_SEVERITY: &[(&str, Severity)] = &[
    ("error", Severity::Error),
    ("warning", Severity::Warning),
    ("info", Severity::Info),
];
const ON_EXCEED: &[(&str, OnExceed)] = &[
    ("warn", OnExceed::Warn),
    ("fail", OnExceed::Fail),
    ("demote", OnExceed::Demote),
];

pub(super) fn parse(yaml: &str) -> Result<Config, ConfigErrors> {
    let root: Value = yaml_serde::from_str(yaml).map_err(|e| {
        ConfigErrors(vec![ConfigError::Syntax {
            message: e.to_string(),
        }])
    })?;
    let mut w = Walker::default();
    w.reject_tags(&root, "");
    let config = w.config(&root);
    match config {
        Some(config) if w.errors.is_empty() => Ok(config),
        _ => Err(ConfigErrors(w.errors)),
    }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

type Fields<'a> = BTreeMap<&'a str, &'a Value>;

#[derive(Default)]
struct Walker {
    errors: Vec<ConfigError>,
}

impl Walker {
    fn invalid(&mut self, path: &str, message: impl Into<String>) {
        self.errors.push(ConfigError::Invalid {
            path: path.to_string(),
            message: message.into(),
        });
    }

    fn wrong_type(&mut self, path: &str, expected: &str) {
        self.errors.push(ConfigError::Type {
            path: path.to_string(),
            expected: expected.to_string(),
        });
    }

    fn missing(&mut self, path: &str, hint: &str) {
        self.errors.push(ConfigError::Missing {
            path: path.to_string(),
            hint: hint.to_string(),
        });
    }

    /// A mapping's entries keyed by name, reporting keys outside `allowed` (all keys when empty).
    fn fields<'a>(&mut self, value: &'a Value, path: &str, allowed: &[&str]) -> Fields<'a> {
        let mut out = Fields::new();
        let Some(map) = self.mapping(value, path) else {
            return out;
        };
        for (key, v) in map {
            let Some(name) = key.as_str() else {
                self.wrong_type(path, "keys that are strings");
                continue;
            };
            if allowed.is_empty() || allowed.contains(&name) {
                out.insert(name, v);
            } else {
                self.errors.push(ConfigError::UnknownKey {
                    path: join(path, name),
                    allowed: allowed.join(", "),
                });
            }
        }
        out
    }

    /// Accessors read through a tag, so a tag anywhere would otherwise be silently dropped.
    fn reject_tags(&mut self, value: &Value, path: &str) {
        match value {
            Value::Tagged(tagged) => {
                let at = if path.is_empty() {
                    "(the document)"
                } else {
                    path
                };
                self.invalid(
                    at,
                    format!(
                        "YAML tags such as `{}` are not allowed; remove it",
                        tagged.tag
                    ),
                );
            }
            Value::Sequence(items) => {
                for (i, item) in items.iter().enumerate() {
                    self.reject_tags(item, &format!("{path}[{i}]"));
                }
            }
            Value::Mapping(map) => {
                for (key, v) in map {
                    let name = key.as_str().map_or_else(|| "?".to_string(), str::to_string);
                    self.reject_tags(key, &join(path, &name));
                    self.reject_tags(v, &join(path, &name));
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }

    fn mapping<'a>(&mut self, value: &'a Value, path: &str) -> Option<&'a Mapping> {
        let map = value.as_mapping();
        if map.is_none() {
            self.wrong_type(path, "a mapping");
        }
        map
    }

    fn string(&mut self, value: &Value, path: &str) -> Option<String> {
        match value.as_str() {
            Some(s) if !s.is_empty() => Some(s.to_string()),
            _ => {
                self.wrong_type(path, "a non-empty string");
                None
            }
        }
    }

    fn strings(&mut self, value: &Value, path: &str) -> Option<Vec<String>> {
        let Some(items) = value.as_sequence() else {
            self.wrong_type(path, "a list of strings");
            return None;
        };
        let mut out = Vec::new();
        for (i, item) in items.iter().enumerate() {
            out.extend(self.string(item, &format!("{path}[{i}]")));
        }
        (out.len() == items.len()).then_some(out)
    }

    fn positive_int(&mut self, value: &Value, path: &str) -> Option<u64> {
        match value.as_u64() {
            Some(n) if n > 0 => Some(n),
            _ => {
                self.wrong_type(path, "a whole number greater than zero");
                None
            }
        }
    }

    fn boolean(&mut self, value: &Value, path: &str) -> Option<bool> {
        let b = value.as_bool();
        if b.is_none() {
            self.wrong_type(path, "true or false");
        }
        b
    }

    fn choice<T: Copy>(&mut self, value: &Value, path: &str, options: &[(&str, T)]) -> Option<T> {
        let found = value
            .as_str()
            .and_then(|s| options.iter().find(|(name, _)| *name == s))
            .map(|(_, t)| *t);
        if found.is_none() {
            let names: Vec<&str> = options.iter().map(|(n, _)| *n).collect();
            self.wrong_type(path, &format!("one of {}", names.join(", ")));
        }
        found
    }

    fn duration(&mut self, value: &Value, path: &str) -> Option<Duration> {
        match value.as_str().map(parse_duration) {
            Some(Ok(d)) => Some(d),
            Some(Err(DurationError::Zero)) => {
                self.invalid(path, DurationError::Zero.to_string());
                None
            }
            Some(Err(DurationError::Format)) | None => {
                self.wrong_type(path, "a duration such as 500ms, 3s, 10m or 1h");
                None
            }
        }
    }

    fn budget(&mut self, value: &Value, path: &str) -> Option<Budget> {
        let f = self.fields(value, path, &["duration"]);
        let Some(d) = f.get("duration") else {
            if value.is_mapping() {
                self.missing(
                    &join(path, "duration"),
                    "a budget needs a duration such as 3s",
                );
            }
            return None;
        };
        self.duration(d, &join(path, "duration"))
            .map(|duration| Budget { duration })
    }

    fn trigger_name(&mut self, value: &Value, path: &str) -> Option<Trigger> {
        let options: Vec<(&str, Trigger)> = Trigger::ALL.iter().map(|t| (t.as_str(), *t)).collect();
        self.choice(value, path, &options)
    }

    fn config(&mut self, root: &Value) -> Option<Config> {
        let f = self.fields(root, "", TOP_KEYS);
        if !root.is_mapping() {
            return None;
        }
        self.schema_version(f.get("schema_version").copied());
        let mut config = Config {
            requires: None,
            triggers: Trigger::ALL
                .iter()
                .map(|t| (*t, TriggerConfig::default_for(*t)))
                .collect(),
            budgets: Budgets {
                on_exceed: OnExceed::Warn,
                regression: None,
            },
            on_crash: OnCrash::Warn,
            timeout_factor: DEFAULT_TIMEOUT_FACTOR,
            receipts_keep: DEFAULT_RECEIPTS_KEEP,
            merge: MergeConfig {
                mode: MergeMode::Status,
                method: None,
            },
            evidence_accept: EvidenceAccept::Ci,
            checks: Vec::new(),
        };
        self.settings(&f, &mut config);
        if let Some(v) = f.get("checks") {
            config.checks = self.checks(v);
        }
        Some(config)
    }

    fn schema_version(&mut self, value: Option<&Value>) {
        match value {
            None => self.missing("schema_version", "add `schema_version: 1`"),
            Some(v) if v.as_u64() == Some(CONFIG_SCHEMA_VERSION) => {}
            Some(v) => self.invalid(
                "schema_version",
                format!(
                    "this garita reads schema_version {CONFIG_SCHEMA_VERSION}, not {}",
                    yaml_serde::to_string(v).unwrap_or_default().trim()
                ),
            ),
        }
    }

    fn settings(&mut self, f: &Fields<'_>, config: &mut Config) {
        if let Some(v) = f.get("requires") {
            config.requires = self.requires(v);
        }
        if let Some(v) = f.get("triggers") {
            self.triggers(v, &mut config.triggers);
        }
        if let Some(v) = f.get("budgets") {
            self.budgets(v, &mut config.budgets);
        }
        if let Some(v) = f.get("on_crash") {
            let options = [("warn", OnCrash::Warn), ("fail", OnCrash::Fail)];
            config.on_crash = self
                .choice(v, "on_crash", &options)
                .unwrap_or(config.on_crash);
        }
        if let Some(v) = f.get("timeout") {
            self.timeout(v, &mut config.timeout_factor);
        }
        if let Some(v) = f.get("receipts") {
            let r = self.fields(v, "receipts", &["keep"]);
            if let Some(keep) = r.get("keep") {
                config.receipts_keep = self
                    .positive_int(keep, "receipts.keep")
                    .unwrap_or(config.receipts_keep);
            }
        }
        if let Some(v) = f.get("merge") {
            self.merge(v, &mut config.merge);
        }
        if let Some(v) = f.get("evidence") {
            self.evidence(v, &mut config.evidence_accept);
        }
    }

    fn requires(&mut self, value: &Value) -> Option<VersionReq> {
        let text = self.string(value, "requires")?;
        match VersionReq::parse(&text) {
            Ok(req) => Some(req),
            Err(e) => {
                self.invalid("requires", e.to_string());
                None
            }
        }
    }

    fn triggers(&mut self, value: &Value, triggers: &mut BTreeMap<Trigger, TriggerConfig>) {
        let names: Vec<&str> = Trigger::ALL.iter().map(|t| t.as_str()).collect();
        let f = self.fields(value, "triggers", &names);
        for trigger in Trigger::ALL {
            let Some(v) = f.get(trigger.as_str()) else {
                continue;
            };
            let path = join("triggers", trigger.as_str());
            let t = self.fields(v, &path, &["scope", "budget", "worktree"]);
            let entry = triggers
                .entry(trigger)
                .or_insert_with(|| TriggerConfig::default_for(trigger));
            if let Some(s) = t.get("scope") {
                let options: Vec<(&str, ScopeKind)> =
                    ScopeKind::ALL.iter().map(|k| (k.as_str(), *k)).collect();
                entry.scope = self
                    .choice(s, &join(&path, "scope"), &options)
                    .unwrap_or(entry.scope);
            }
            if let Some(b) = t.get("budget") {
                entry.budget = self.budget(b, &join(&path, "budget"));
            }
            if let Some(w) = t.get("worktree") {
                let options = [("current", Worktree::Current), ("merged", Worktree::Merged)];
                entry.worktree = self
                    .choice(w, &join(&path, "worktree"), &options)
                    .unwrap_or(entry.worktree);
            }
        }
    }

    fn budgets(&mut self, value: &Value, budgets: &mut Budgets) {
        let f = self.fields(value, "budgets", &["on_exceed", "regression"]);
        if let Some(v) = f.get("on_exceed") {
            budgets.on_exceed = self
                .choice(v, "budgets.on_exceed", ON_EXCEED)
                .unwrap_or(budgets.on_exceed);
        }
        if let Some(v) = f.get("regression") {
            budgets.regression = Some(self.regression(v));
        }
    }

    fn regression(&mut self, value: &Value) -> Regression {
        let path = "budgets.regression";
        let f = self.fields(value, path, &["window", "threshold"]);
        let window = f
            .get("window")
            .and_then(|v| self.positive_int(v, &join(path, "window")));
        let threshold_percent = f.get("threshold").and_then(|v| {
            let parsed = v
                .as_str()
                .and_then(|s| s.strip_suffix('%'))
                .and_then(|n| n.parse::<f64>().ok())
                .filter(|n| n.is_finite() && *n >= 0.0);
            if parsed.is_none() {
                self.wrong_type(&join(path, "threshold"), "a percentage such as 25%");
            }
            parsed
        });
        Regression {
            window,
            threshold_percent,
        }
    }

    fn timeout(&mut self, value: &Value, factor: &mut f64) {
        let f = self.fields(value, "timeout", &["factor"]);
        if let Some(v) = f.get("factor") {
            match v.as_f64().filter(|n| n.is_finite() && *n > 0.0) {
                Some(n) => *factor = n,
                None => self.wrong_type("timeout.factor", "a number greater than zero"),
            }
        }
    }

    fn merge(&mut self, value: &Value, merge: &mut MergeConfig) {
        let f = self.fields(value, "merge", &["mode", "method"]);
        if let Some(v) = f.get("mode") {
            let options = [
                ("admin", MergeMode::Admin),
                ("status", MergeMode::Status),
                ("auto", MergeMode::Auto),
            ];
            merge.mode = self.choice(v, "merge.mode", &options).unwrap_or(merge.mode);
        }
        if let Some(v) = f.get("method") {
            merge.method = self.string(v, "merge.method");
        }
    }

    fn evidence(&mut self, value: &Value, accept: &mut EvidenceAccept) {
        let f = self.fields(value, "evidence", &["accept"]);
        if let Some(v) = f.get("accept") {
            let options = [
                ("ci", EvidenceAccept::Ci),
                ("local", EvidenceAccept::Local),
                ("any", EvidenceAccept::Any),
            ];
            *accept = self
                .choice(v, "evidence.accept", &options)
                .unwrap_or(*accept);
        }
    }

    fn checks(&mut self, value: &Value) -> Vec<Check> {
        let Some(map) = self.mapping(value, "checks") else {
            return Vec::new();
        };
        let mut checks = Vec::new();
        for (key, v) in map {
            match key.as_str() {
                Some(RESERVED_CHECK) => self.invalid(
                    &join("checks", RESERVED_CHECK),
                    "the name `garita` is reserved for findings about the run itself; rename the check",
                ),
                Some(name) if is_check_name(name) => checks.extend(self.check(name, v)),
                Some(name) => self.invalid(
                    &join("checks", name),
                    "a check name starts with a letter or digit and holds only letters, digits, `-`, `_` and `.`",
                ),
                None => self.wrong_type("checks", "check names that are strings"),
            }
        }
        checks
    }

    fn check(&mut self, name: &str, value: &Value) -> Option<Check> {
        let path = join("checks", name);
        let f = self.fields(value, &path, CHECK_KEYS);
        if !value.is_mapping() {
            return None;
        }
        let form = self.form(&f, &path);
        let list = |w: &mut Self, key: &str| {
            f.get(key)
                .and_then(|v| w.strings(v, &join(&path, key)))
                .unwrap_or_default()
        };
        let globs = list(self, "globs");
        let exclude = list(self, "exclude");
        let partition = f
            .get("partition")
            .and_then(|v| self.partition(v, &join(&path, "partition")));
        let via = f
            .get("via")
            .and_then(|v| self.string(v, &join(&path, "via")));
        let cwd = f.get("cwd").and_then(|v| self.cwd(v, &join(&path, "cwd")));
        let kind = f.get("kind").and_then(|v| {
            let options: Vec<(&str, CheckKind)> =
                CheckKind::ALL.iter().map(|k| (k.as_str(), *k)).collect();
            self.choice(v, &join(&path, "kind"), &options)
        });
        let budget = f
            .get("budget")
            .and_then(|v| self.budget(v, &join(&path, "budget")));
        let on_exceed = f
            .get("on_exceed")
            .and_then(|v| self.choice(v, &join(&path, "on_exceed"), ON_EXCEED));
        let on = self.on(f.get("on").copied(), &join(&path, "on"));
        let advisory = f
            .get("advisory")
            .and_then(|v| self.boolean(v, &join(&path, "advisory")))
            .unwrap_or(false);
        let min_severity = f
            .get("min_severity")
            .and_then(|v| self.choice(v, &join(&path, "min_severity"), MIN_SEVERITY))
            .unwrap_or(Severity::Info);
        Some(Check {
            name: name.to_string(),
            form: form?,
            globs,
            exclude,
            partition,
            via,
            cwd,
            kind,
            budget,
            on_exceed,
            on: on?,
            advisory,
            min_severity,
        })
        .inspect(|check| self.check_invocation(check, &path))
    }

    fn cwd(&mut self, value: &Value, path: &str) -> Option<String> {
        let dir = self.string(value, path)?;
        let segments: Vec<&str> = dir
            .split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .collect();
        if dir.starts_with('/') || dir.contains('\\') || segments.contains(&"..") {
            self.invalid(
                path,
                "a repository-relative directory with `/` separators and no `..`",
            );
            return None;
        }
        (!segments.is_empty()).then(|| segments.join("/"))
    }

    fn check_invocation(&mut self, check: &Check, path: &str) {
        let CheckForm::Run { command, .. } = &check.form else {
            return;
        };
        if let Some(via) = check.via.as_deref().filter(|v| *v != VIA_TURBO) {
            let message =
                format!("`{via}` is not a task runner garita knows; the only one is `turbo`");
            self.invalid(&join(path, "via"), message);
        }
        if check.cwd.is_some() && check.partition.is_some() {
            let message = "a check runs in one `cwd` or once per `partition` root, not both";
            self.invalid(&join(path, "cwd"), message);
        }
        if command.placeholders().contains(&Placeholder::Affected) && check.via.is_none() {
            let message = "`{affected}` is filled by a task runner; add `via: turbo`";
            self.invalid(&join(path, "run"), message);
        }
    }

    fn form(&mut self, f: &Fields<'_>, path: &str) -> Option<CheckForm> {
        let present: Vec<&str> = ["run", "native", "builtin"]
            .into_iter()
            .filter(|k| f.contains_key(k))
            .collect();
        match (
            present.as_slice(),
            f.get(present.first().copied().unwrap_or("run")),
        ) {
            (["run"], Some(run)) => self.run_form(run, f.get("parser").copied(), path),
            (["native"], Some(native)) => {
                let note = "a native check reads the tool's report itself";
                self.forbid(f, path, &["parser"], note);
                let tool = self.string(native, &join(path, "native"))?;
                Some(CheckForm::Native { tool })
            }
            (["builtin"], Some(builtin)) => {
                let note = "a builtin check takes no parser and no input";
                self.forbid(f, path, BUILTIN_FORBIDS, note);
                self.builtin(builtin, &join(path, "builtin"))
            }
            ([], _) => {
                let hint = "a check needs `run` (with an optional `parser`), `native` or `builtin`";
                self.missing(&join(path, "run"), hint);
                None
            }
            (many, _) => {
                let names: Vec<String> = many.iter().map(|k| format!("`{k}`")).collect();
                let message = format!(
                    "declare only one of `run`, `native` or `builtin`, not {}",
                    names.join(" and ")
                );
                self.invalid(path, message);
                None
            }
        }
    }

    fn forbid(&mut self, f: &Fields<'_>, path: &str, keys: &[&str], note: &str) {
        for key in keys.iter().filter(|k| f.contains_key(*k)) {
            self.invalid(&join(path, key), format!("{note}; remove `{key}`"));
        }
    }

    fn builtin(&mut self, value: &Value, path: &str) -> Option<CheckForm> {
        let name = self.string(value, path)?;
        match Builtin::ALL.into_iter().find(|b| b.as_str() == name) {
            Some(builtin) => Some(CheckForm::Builtin { builtin }),
            None => {
                let known: Vec<&str> = Builtin::ALL.iter().map(|b| b.as_str()).collect();
                let message = format!(
                    "`{name}` is not a builtin check; the known builtins are {}",
                    known.join(", ")
                );
                self.invalid(path, message);
                None
            }
        }
    }

    fn run_form(&mut self, run: &Value, parser: Option<&Value>, path: &str) -> Option<CheckForm> {
        let parser = match parser {
            Some(p) => self.string(p, &join(path, "parser")),
            None => Some(DEFAULT_PARSER.to_string()),
        };
        let run_path = join(path, "run");
        let run = self.string(run, &run_path)?;
        let command = match Command::parse(&run) {
            Ok(command) => command,
            Err(e) => {
                self.invalid(&run_path, e.to_string());
                return None;
            }
        };
        if matches!(command.args().first(), Some(Arg::Placeholder(_))) {
            self.invalid(
                &run_path,
                "the command must start with a program, not a placeholder",
            );
            return None;
        }
        Some(CheckForm::Run {
            run,
            command,
            parser: parser?,
        })
    }

    fn partition(&mut self, value: &Value, path: &str) -> Option<Partition> {
        let f = self.fields(value, path, &["by", "roots"]);
        if !value.is_mapping() {
            return None;
        }
        let by_ok = match f.get("by") {
            Some(by) => self.choice(by, &join(path, "by"), &[("dir", ())]).is_some(),
            None => {
                self.missing(&join(path, "by"), "set `by: dir`");
                false
            }
        };
        let roots = match f.get("roots") {
            Some(r) => self.strings(r, &join(path, "roots")),
            None => {
                self.missing(
                    &join(path, "roots"),
                    "list the root globs, such as [\"packages/*\"]",
                );
                None
            }
        };
        match roots {
            Some(roots) if roots.is_empty() => {
                self.invalid(&join(path, "roots"), "list at least one root");
                None
            }
            Some(roots) if by_ok => Some(Partition { roots }),
            _ => None,
        }
    }

    fn on(&mut self, value: Option<&Value>, path: &str) -> Option<Vec<Trigger>> {
        let Some(value) = value else {
            return Some(Trigger::ALL.to_vec());
        };
        let Some(items) = value.as_sequence() else {
            self.wrong_type(path, "a list of triggers: commit, push, ci, merge");
            return None;
        };
        if items.is_empty() {
            self.invalid(path, "list at least one trigger, or remove the check");
            return None;
        }
        let mut on = Vec::new();
        let mut ok = true;
        for (i, item) in items.iter().enumerate() {
            let item_path = format!("{path}[{i}]");
            match self.trigger_name(item, &item_path) {
                Some(t) if on.contains(&t) => {
                    self.invalid(&item_path, format!("`{}` is listed twice", t.as_str()));
                    ok = false;
                }
                Some(t) => on.push(t),
                None => ok = false,
            }
        }
        on.sort();
        ok.then_some(on)
    }
}

fn is_check_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}
