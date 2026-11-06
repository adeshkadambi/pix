//! Enemy role inference (SPEC §5.2): enemy positions are hidden in champ select, so score every
//! assignment of distinct roles by the product of each champion's role rate and pick the best.

use serde::Serialize;
use ts_rs::TS;

use super::Role;

/// Off-role rate floor, so a champion played in an unusual role still gets an assignment.
const FLOOR: f32 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct InferredRole {
    pub role: Role,
    /// Share of the total score held by assignments that give this enemy this role (0–1).
    pub confidence: f32,
    /// Set by the user rather than inferred.
    pub manual: bool,
}

fn rate(rates: &[(Role, f32)], role: Role) -> f32 {
    rates.iter().find(|(r, _)| *r == role).map(|(_, x)| *x).unwrap_or(0.0).max(FLOOR)
}

/// All ways to give `n` enemies distinct roles.
fn assignments(n: usize) -> Vec<Vec<Role>> {
    fn go(n: usize, current: &mut Vec<Role>, out: &mut Vec<Vec<Role>>) {
        if current.len() == n {
            out.push(current.clone());
            return;
        }
        for role in Role::ALL {
            if !current.contains(&role) {
                current.push(role);
                go(n, current, out);
                current.pop();
            }
        }
    }
    let mut out = Vec::new();
    go(n.min(5), &mut Vec::new(), &mut out);
    out
}

/// `rates[i]` are enemy i's role rates; `fixed[i]` is a user override for enemy i.
pub fn infer(rates: &[Vec<(Role, f32)>], fixed: &[Option<Role>]) -> Vec<InferredRole> {
    let n = rates.len().min(5);
    if n == 0 {
        return Vec::new();
    }
    let scored: Vec<(Vec<Role>, f32)> = assignments(n)
        .into_iter()
        .filter(|a| a.iter().enumerate().all(|(i, r)| fixed.get(i).copied().flatten().is_none_or(|f| f == *r)))
        .map(|a| {
            let score = a.iter().enumerate().map(|(i, r)| rate(&rates[i], *r)).product();
            (a, score)
        })
        .collect();
    let total: f32 = scored.iter().map(|(_, s)| s).sum();
    let Some((best, _)) = scored.iter().max_by(|a, b| a.1.total_cmp(&b.1)) else {
        // Conflicting overrides (two enemies forced into one role): fall back to the overrides.
        return (0..n)
            .map(|i| InferredRole { role: fixed.get(i).copied().flatten().unwrap_or(Role::ALL[i]), confidence: 0.0, manual: true })
            .collect();
    };
    best.iter()
        .enumerate()
        .map(|(i, role)| {
            let agree: f32 = scored.iter().filter(|(a, _)| a[i] == *role).map(|(_, s)| s).sum();
            InferredRole {
                role: *role,
                confidence: if total > 0.0 { agree / total } else { 0.0 },
                manual: fixed.get(i).copied().flatten().is_some(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use Role::*;

    fn roles(v: &[InferredRole]) -> Vec<Role> {
        v.iter().map(|r| r.role).collect()
    }

    #[test]
    fn counts_assignments() {
        assert_eq!(assignments(5).len(), 120);
        assert_eq!(assignments(2).len(), 20);
    }

    #[test]
    fn obvious_comp() {
        let rates = vec![
            vec![(Top, 0.9), (Mid, 0.1)],          // Malphite
            vec![(Jungle, 0.95)],                  // Viego
            vec![(Mid, 0.97)],                     // Syndra
            vec![(Adc, 0.98)],                     // Kai'Sa
            vec![(Support, 0.99)],                 // Leona
        ];
        let out = infer(&rates, &[]);
        assert_eq!(roles(&out), vec![Top, Jungle, Mid, Adc, Support]);
        assert!(out.iter().all(|r| r.confidence > 0.8 && !r.manual));
    }

    #[test]
    fn flex_picks_resolve_around_each_other() {
        // Two mid/top flex picks: the one that's more often top goes top.
        let rates = vec![vec![(Mid, 0.6), (Top, 0.4)], vec![(Mid, 0.9), (Top, 0.1)]];
        let out = infer(&rates, &[]);
        assert_eq!(roles(&out), vec![Top, Mid]);
        assert!(out[0].confidence < 0.9, "less sure about the flex pick");
    }

    #[test]
    fn overrides_win_and_reshape_the_rest() {
        let rates = vec![vec![(Mid, 0.6), (Top, 0.4)], vec![(Mid, 0.9), (Top, 0.1)]];
        let out = infer(&rates, &[None, Some(Top)]);
        assert_eq!(roles(&out), vec![Mid, Top]);
        assert!(out[1].manual && !out[0].manual);
    }

    #[test]
    fn off_role_champions_still_get_a_role() {
        let out = infer(&[vec![(Adc, 1.0)], vec![(Adc, 1.0)]], &[]);
        assert_eq!(out.len(), 2);
        assert_ne!(out[0].role, out[1].role);
    }

    #[test]
    fn empty() {
        assert!(infer(&[], &[]).is_empty());
    }
}
