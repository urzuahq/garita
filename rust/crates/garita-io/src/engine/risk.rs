//! The change's risk class, judged by the merge base's policy (RFC-3 §3).

use std::path::Path;

use garita_core::ObjectId;
use garita_core::config::Config;
use garita_core::report::{Finding, Risk, Severity};

use super::finding::run_finding;
use super::scope;
use crate::config_file;
use crate::git;

/// The report's `risk`, and a finding saying why the class could not be read when it could not.
pub(crate) struct Judged {
    pub risk: Option<Risk>,
    pub finding: Option<Finding>,
}

impl Judged {
    fn unreadable(why: &str) -> Self {
        let message = format!("the change has no risk class, so a person must approve it: {why}");
        Self {
            risk: Some(Risk::base_unreadable()),
            finding: Some(run_finding(Severity::Info, "risk-unreadable", message)),
        }
    }

    const NO_POLICY: Judged = Judged {
        risk: None,
        finding: None,
    };
}

/// Matches the change against the policy at its merge base with `base` (chosen as the `changed`
/// scope chooses it). `staged` judges the index rather than HEAD, against HEAD when no base ref
/// is found, since a staged change cannot alter HEAD's policy. Without a merge base the class is
/// unreadable when the head declares a policy, and there is no policy when it does not.
pub(crate) fn judge(root: &Path, base: Option<&str>, staged: bool, head_declares: bool) -> Judged {
    let found = scope::merge_base(root, base).and_then(|found| match found {
        Err(_) if staged && base.is_none() => {
            Ok(git::head_commit(root)?.ok_or_else(|| "there is no commit yet".to_string()))
        }
        found => Ok(found.map(|(_, merge_base)| merge_base)),
    });
    let merge_base = match found {
        Ok(Ok(merge_base)) => merge_base,
        Ok(Err(why)) if head_declares => {
            return Judged::unreadable(&format!("no merge base, so no policy to judge by: {why}"));
        }
        Err(e) if head_declares => {
            return Judged::unreadable(&format!("no merge base, so no policy to judge by: {e}"));
        }
        _ => return Judged::NO_POLICY,
    };
    match at_merge_base(root, &merge_base, staged) {
        Ok(risk) => Judged {
            risk,
            finding: None,
        },
        Err(why) => Judged::unreadable(&why),
    }
}

fn at_merge_base(root: &Path, merge_base: &ObjectId, staged: bool) -> Result<Option<Risk>, String> {
    let path = config_file::PATH;
    let at = format!("{path} at the merge base {}", short(merge_base));
    let text = match git::show_file(root, merge_base.as_str(), path) {
        Ok(Some(text)) => text,
        Ok(None) => return Ok(None),
        Err(e) => return Err(format!("could not read {at}: {e}")),
    };
    if !Config::may_declare_risk(&text) {
        return Ok(None);
    }
    let config = Config::parse(&text).map_err(|e| format!("{at} is not a valid config: {e}"))?;
    let Some(policy) = config.risk else {
        return Ok(None);
    };
    let touched = git::touched_since(root, merge_base, staged)
        .map_err(|e| format!("could not list the paths the change touches: {e}"))?;
    Ok(Some(policy.report(policy.match_change(&touched))))
}

fn short(id: &ObjectId) -> &str {
    id.as_str().get(..12).unwrap_or(id.as_str())
}
