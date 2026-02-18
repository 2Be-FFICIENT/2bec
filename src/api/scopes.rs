//! Which scope each operation needs, from the `x-scopes` of the API's
//! OpenAPI document. A `scope-missing` answer does not name the scope, so
//! the CLI names it from this table.

/// `{}` matches one path segment. Paths are without a leading slash.
const TABLE: &[(&str, &str, &str)] = &[
    ("GET", "v2/run-requests", "runs:request"),
    ("GET", "v2/run-requests/{}", "runs:request"),
    ("GET", "v2/runs", "runs:read"),
    ("GET", "v2/runs/facets", "runs:read"),
    ("GET", "v2/runs/{}", "runs:read"),
    ("GET", "v2/runs/{}/artifacts", "runs:read"),
    ("GET", "v2/runs/{}/artifacts/{}", "runs:read"),
    ("GET", "v2/runs/{}/call", "runs:read"),
    ("GET", "v2/runs/{}/comments", "runs:read"),
    ("POST", "v2/runs/{}/comments", "runs:write"),
    ("GET", "v2/runs/{}/comments/{}", "runs:read"),
    ("PATCH", "v2/runs/{}/comments/{}", "runs:write"),
    ("DELETE", "v2/runs/{}/comments/{}", "runs:write"),
    ("GET", "v2/runs/{}/diagnosis", "runs:read"),
    ("GET", "v2/runs/{}/requests", "runs:read"),
    ("POST", "v2/runs/{}/tags", "runs:write"),
    ("DELETE", "v2/runs/{}/tags/{}", "runs:write"),
    ("POST", "v2/runs:tag", "runs:write"),
    ("GET", "v2/scenario-groups", "scenarios:read"),
    ("GET", "v2/scenario-groups/{}", "scenarios:read"),
    ("GET", "v2/scenarios", "scenarios:read"),
    ("GET", "v2/scenarios/{}", "scenarios:read"),
    ("PATCH", "v2/scenarios/{}", "scenarios:write"),
    ("GET", "v2/scenarios/{}/calendar", "scenarios:read"),
    ("GET", "v2/scenarios/{}/documentation.pdf", "scenarios:read"),
    ("GET", "v2/scenarios/{}/response-times", "metrics:read"),
    ("GET", "v2/scenarios/{}/run-requests", "runs:request"),
    ("POST", "v2/scenarios/{}/run-requests", "runs:request"),
    ("PUT", "v2/scenarios/{}/run-variables", "scenarios:write"),
    ("GET", "v2/scenarios/{}/runs", "runs:read"),
    ("GET", "v2/scenarios/{}/settings", "scenarios:read"),
    ("PUT", "v2/scenarios/{}/steps", "scenarios:write"),
    ("GET", "v2/scenarios/{}/synthesis", "metrics:read"),
    ("GET", "v2/scenarios/{}/thresholds", "metrics:read"),
    (
        "POST",
        "v2/scenarios/{}/thresholds:apply",
        "indicators:write",
    ),
    (
        "POST",
        "v2/scenarios/{}/thresholds:compute",
        "indicators:write",
    ),
    ("GET", "v2/scenarios/{}/versions", "scenarios:read"),
    ("GET", "v2/scenarios/{}/voice-quality", "metrics:read"),
    ("POST", "v2/scenarios/{}:pause", "monitoring:write"),
    ("POST", "v2/scenarios/{}:resume", "monitoring:write"),
];

/// The scope `method path` needs, when the table knows the operation.
pub fn required(method: &str, path: &str) -> Option<&'static str> {
    let path = path.trim_start_matches('/');
    let path = path.split('?').next().unwrap_or(path);
    TABLE
        .iter()
        .find(|(m, pattern, _)| m.eq_ignore_ascii_case(method) && matches(pattern, path))
        .map(|(_, _, scope)| *scope)
}

fn matches(pattern: &str, path: &str) -> bool {
    let mut p = pattern.split('/');
    let mut s = path.split('/');
    loop {
        match (p.next(), s.next()) {
            (None, None) => return true,
            (Some("{}"), Some(seg)) if !seg.is_empty() => {}
            (Some(a), Some(b)) if a == b => {}
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::required;

    #[test]
    fn known_operations() {
        assert_eq!(required("GET", "v2/runs"), Some("runs:read"));
        assert_eq!(required("get", "/v2/runs?limit=5"), Some("runs:read"));
        assert_eq!(
            required("DELETE", "v2/runs/abc/comments/def"),
            Some("runs:write")
        );
        assert_eq!(
            required("POST", "v2/scenarios/abc/run-requests"),
            Some("runs:request")
        );
        assert_eq!(
            required("GET", "v2/scenarios/abc/documentation.pdf?technical=true"),
            Some("scenarios:read")
        );
        assert_eq!(required("GET", "v2/unknown"), None);
        assert_eq!(required("GET", "v2/runs/abc/extra/deep"), None);
    }
}
