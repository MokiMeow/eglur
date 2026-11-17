use super::{diagnostic_codes, ownership_profile, session};
use eglur_driver::TrustedCatalog;
use eglur_sema::{FactState, OwnershipBodyStatus, PhaseProduct};

fn checked(source: &str, expected_codes: &[&str]) {
    let mut session = session(source);
    let catalog = TrustedCatalog::first_slice().unwrap();
    let analysis = session
        .resolve(&ownership_profile(), Some(&catalog))
        .unwrap();
    let resolution = analysis
        .resolution()
        .expect("ownership analysis is selected");
    let PhaseProduct::Performed(facts) = resolution.ownership_facts() else {
        panic!("ownership facts must be performed");
    };
    assert!(
        facts.is_complete(),
        "incomplete ownership analysis: {source}"
    );
    assert!(facts.fact_count() > 0);
    for (_, fact) in facts.bodies() {
        assert!(
            matches!(fact, FactState::Known(body) if body.status() == OwnershipBodyStatus::Checked),
            "unexpected body fact {fact:?}: {source}",
        );
    }
    assert_eq!(
        diagnostic_codes(&session.into_diagnostics(None)),
        expected_codes,
        "{source}"
    );
}

#[test]
fn parser_budgeted_groups_keep_checked_ownership_on_the_default_stack() {
    checked(
        &format!(
            "fn f(x: i64) -> i64 = {}x{}\n",
            "(".repeat(255),
            ")".repeat(255)
        ),
        &[],
    );
}

#[test]
fn deeply_grouped_named_root_preserves_missing_move_recovery() {
    checked(
        &format!(
            "fn f(x: Own[i64]) -> Own[i64] = {}x{}\n",
            "(".repeat(255),
            ")".repeat(255),
        ),
        &["OWNERSHIP_TRANSFER_REQUIRES_MOVE"],
    );
}

#[test]
fn deeply_grouped_borrow_keeps_its_consumed_referent() {
    checked(
        &format!(
            concat!(
                "fn peek(x: &Own[i64]) -> i64 = 0\n",
                "fn f(x: Own[i64]) -> i64 {{\n",
                "  let alias = &{}x{}\n",
                "  drop_own move x\n",
                "  return peek(alias)\n",
                "}}\n",
            ),
            "(".repeat(250),
            ")".repeat(250),
        ),
        &["OWNERSHIP_USE_AFTER_DISCHARGE"],
    );
}

#[test]
fn nested_if_branches_join_only_continuing_ownership_states() {
    checked(
        concat!(
            "fn f(x: Own[i64], flag: Bool) -> i64 {\n",
            "  if flag {\n",
            "    if flag {\n",
            "      drop_own move x\n",
            "      return 1\n",
            "    } else { yield () }\n",
            "    yield ()\n",
            "  } else { yield () }\n",
            "  drop_own move x\n",
            "  return 0\n",
            "}\n",
        ),
        &[],
    );
    checked(
        concat!(
            "fn f(x: Own[i64], flag: Bool) -> i64 {\n",
            "  if flag {\n",
            "    drop_own move x\n",
            "    return 1\n",
            "  } else {\n",
            "    drop_own move x\n",
            "    return 2\n",
            "  }\n",
            "  let unreachable = move x\n",
            "  return 0\n",
            "}\n",
        ),
        &[],
    );
}

#[test]
fn nested_result_and_checked_exits_preserve_outer_obligations() {
    for source in [
        concat!(
            "fn f(x: Own[i64], source: Res[i64, Str]) -> i64 {\n",
            "  let outcome: Res[i64, Str] = result {\n",
            "    let nested: Res[i64, Str] = result { yield Ok(source?) }\n",
            "    yield Ok(nested?)\n",
            "  }\n",
            "  drop_own move x\n",
            "  return 0\n",
            "}\n",
        ),
        concat!(
            "fn f(x: Own[i64], value: i8) -> i64 {\n",
            "  let outcome: Res[i8, NumericError] = result {\n",
            "    let nested: Res[i8, NumericError] = checked { yield value + value }\n",
            "    yield Ok(nested?)\n",
            "  }\n",
            "  drop_own move x\n",
            "  return 0\n",
            "}\n",
        ),
    ] {
        checked(source, &[]);
    }
}
