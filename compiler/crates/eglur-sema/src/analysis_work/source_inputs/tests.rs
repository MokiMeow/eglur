use eglur_hir::{ItemKind, TypeRefId};
use eglur_source::SourceFile;

use crate::record_reservation::observation::{assert_growth_envelope, observe};

use super::*;

const ALLOWANCE: u64 = 1_000_000;
const FILE: FileId = FileId::new(3);

fn parse(text: &str) -> HirFile {
    let raw = SourceFile::new(FILE, text.as_bytes().to_vec());
    let parsed = eglur_syntax::parse_validated(raw.validate().unwrap());
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    let hir = eglur_hir::lower(&parsed);
    assert!(hir.invalid_occurrences().is_empty());
    hir
}

fn accounting() -> AnalysisWorkAccounting {
    AnalysisWorkAccounting::new(AnalysisWorkPricing::for_test(7, 2, 3).unwrap(), 3).unwrap()
}

fn registry<'hir>(
    accounting: &AnalysisWorkAccounting,
    hir: &'hir HirFile,
) -> AnalysisSourceInputRegistry<'hir> {
    AnalysisSourceInputRegistry::new(accounting, 0, FILE, hir).unwrap()
}

fn site(type_ref: TypeRefId) -> TypeOccurrenceId {
    TypeOccurrenceId::new(FILE, type_ref)
}

fn alias_rhs(hir: &HirFile, name: &str) -> TypeRefId {
    hir.items()
        .find_map(|(_, item)| match item.kind() {
            ItemKind::TypeAlias(alias) if alias.name.text() == Some(name) => Some(alias.value),
            _ => None,
        })
        .unwrap()
}

fn incomplete(error: AnalysisSourceInputFailure) -> AnalysisWorkIncomplete {
    let AnalysisSourceInputFailure::Work(AnalysisWorkFailure::Incomplete(stop)) = error else {
        panic!("typed incomplete expected: {error:?}")
    };
    stop
}

fn original_sites(inputs: &AnalysisSourceInputRegistry<'_>) -> Vec<TypeOccurrenceId> {
    inputs
        .hir
        .type_refs()
        .filter(|(id, _)| matches!(inputs.slots[id.get() as usize], SourceTypeSlot::Original(_)))
        .map(|(id, _)| TypeOccurrenceId::new(inputs.file, id))
        .collect()
}

#[test]
fn empty_and_non_type_sources_prepare_no_roots() {
    for text in ["", "struct Empty {}"] {
        let hir = parse(text);
        let mut work = accounting();
        let inputs = registry(&work, &hir);
        assert!(inputs.slots.is_empty());
        assert!(inputs.tickets.is_empty());
        assert_eq!(
            work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
            0
        );
    }
}

#[test]
fn setup_indexes_source_nodes_without_admitting_any_written_head() {
    let hir = parse("type Pair[T] = (T, T)\nfn inspect(x: Pair[i32]) -> Pair[i32] = x");
    let mut work = accounting();
    let inputs = registry(&work, &hir);
    assert_eq!(inputs.slots.len(), hir.type_refs().len());
    assert!(inputs.tickets.is_empty());
    assert!(
        inputs
            .slots
            .iter()
            .all(|slot| !matches!(slot, SourceTypeSlot::Original(Some(_))))
    );
    assert_eq!(
        work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
        0
    );
}

#[test]
fn every_structural_type_family_has_one_original_head_not_child_allowances() {
    for spelling in [
        "i32",
        "(i32, Bool)",
        "((i32, Bool), (Str, f32))",
        "(i32)",
        "&i32",
        "&mut i32",
        "*const i32",
        "*mut &Bool",
        "dyn api::View[i32]",
        "Opt[i32]",
        "Res[(i32, Bool), Str]",
        "Fn() -> i32",
        "Fn(i32, Opt[Bool]) -> (Str, f32)",
        "Fn(Fn(i32) -> Bool) -> Fn(Str) -> f32",
    ] {
        let hir = parse(&format!("type Input = {spelling}"));
        let head = alias_rhs(&hir, "Input");
        let mut work = accounting();
        let mut inputs = registry(&work, &hir);
        assert_eq!(original_sites(&inputs), [site(head)], "{spelling}");
        let ticket = inputs
            .written_ticket(&mut work, site(head), ALLOWANCE)
            .unwrap();
        for (id, _) in hir.type_refs().filter(|(id, _)| *id != head) {
            assert!(
                matches!(
                    inputs.written_ticket(&mut work, site(id), ALLOWANCE),
                    Err(AnalysisSourceInputFailure::NotOriginalType(found)) if found == site(id)
                ),
                "{spelling}"
            );
            assert_eq!(inputs.inherited_ticket(&mut work, &ticket).unwrap(), ticket);
        }
        assert_eq!(inputs.tickets.len(), 1);
        assert_eq!(
            work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
            1
        );
    }
}

#[test]
fn alias_rhs_repeated_uses_and_different_instantiations_are_distinct_originals() {
    let hir = parse(
        "type Pair[T] = (T, T)\nfn inspect(a: Pair[i32], b: Pair[i32], c: Pair[Bool]) -> Pair[i32] = a",
    );
    let mut work = accounting();
    let mut inputs = registry(&work, &hir);
    let heads = original_sites(&inputs);
    assert_eq!(heads.len(), 5);
    assert!(heads.contains(&site(alias_rhs(&hir, "Pair"))));
    let tickets = heads
        .iter()
        .map(|&head| inputs.written_ticket(&mut work, head, ALLOWANCE).unwrap())
        .collect::<Vec<_>>();
    for (index, ticket) in tickets.iter().enumerate() {
        assert!(tickets[..index].iter().all(|previous| previous != ticket));
    }

    let alias_children = hir
        .type_refs()
        .filter(|(id, _)| !heads.contains(&site(*id)))
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    for ticket in &tickets {
        for _ in &alias_children {
            assert_eq!(&inputs.inherited_ticket(&mut work, ticket).unwrap(), ticket);
        }
    }
    assert_eq!(inputs.tickets.len(), heads.len());
    assert_eq!(
        work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
        heads.len()
    );
}

#[test]
fn bounds_fields_associated_bindings_annotations_and_explicit_type_inputs_are_heads() {
    let hir = parse(
        "trait Tr[A] { type Item }\nstruct Box[T: Tr[i32]] { field item: Opt[T] }\nimpl Tr[Bool] for Box[i32] { type Item = (i32, Bool) }\nfn inspect(x: i32) -> Unit {\nlet y: (i32, Bool) = (x, true)\nconsume::[Opt[i32], Fn(i32) -> Bool](y)\nyield ()\n}",
    );
    let mut work = accounting();
    let mut inputs = registry(&work, &hir);
    let expected = hir
        .items()
        .flat_map(|(_, item)| match item.kind() {
            ItemKind::Struct(value) => value
                .generic_parameters
                .iter()
                .flat_map(|parameter| parameter.bounds.iter().copied())
                .collect(),
            ItemKind::StorageField(value) => vec![value.ty],
            ItemKind::Impl(value) => vec![value.trait_ref, value.target],
            ItemKind::AssociatedTypeBinding(value) => vec![value.value],
            ItemKind::Function(value) => value
                .parameters
                .iter()
                .filter_map(|parameter| parameter.ty)
                .chain(value.return_type)
                .collect(),
            _ => Vec::new(),
        })
        .chain(hir.bodies().flat_map(|(_, body)| {
            body.statements()
                .filter_map(|(_, statement)| match statement.kind() {
                    eglur_hir::StmtKind::Let { ty, .. } => *ty,
                    _ => None,
                })
                .chain(
                    body.expressions()
                        .flat_map(|(_, expression)| match expression.kind() {
                            eglur_hir::ExprKind::Call(call) => call.type_arguments.to_vec(),
                            _ => Vec::new(),
                        }),
                )
                .collect::<Vec<_>>()
        }))
        .map(site)
        .collect::<Vec<_>>();
    assert_eq!(original_sites(&inputs).len(), expected.len());
    for head in expected {
        inputs.written_ticket(&mut work, head, ALLOWANCE).unwrap();
    }
    assert_eq!(inputs.tickets.len(), original_sites(&inputs).len());
}

#[test]
fn catalog_dtype_and_lambda_inputs_remain_written_heads() {
    let hir = parse(
        "fn inspect(x: i32) -> Unit {\nStore[Opt[i32], Bool].lookup(x)\nzeros[f32](1)\nlet callback = { |value: Opt[i32]| value }\nyield ()\n}",
    );
    let mut work = accounting();
    let mut inputs = registry(&work, &hir);
    let mut heads = Vec::new();
    for (_, body) in hir.bodies() {
        for (_, expression) in body.expressions() {
            match expression.kind() {
                eglur_hir::ExprKind::CatalogCall(call) => {
                    heads.extend(call.owner_arguments.iter().copied());
                }
                eglur_hir::ExprKind::DTypeCall(call) => heads.push(call.dtype),
                eglur_hir::ExprKind::Lambda(lambda) => {
                    heads.extend(
                        lambda
                            .parameters
                            .iter()
                            .filter_map(|parameter| parameter.ty),
                    );
                }
                _ => {}
            }
        }
    }
    assert_eq!(heads.len(), 4);
    for head in heads {
        inputs
            .written_ticket(&mut work, site(head), ALLOWANCE)
            .unwrap();
    }
    assert_eq!(inputs.tickets.len(), 4);
    assert_eq!(
        work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
        4
    );
}

#[test]
fn replay_and_owned_ticket_clones_keep_one_balance_across_phases() {
    let hir = parse("type Input = (i32, Opt[Bool])");
    let head = site(alias_rhs(&hir, "Input"));
    let mut work = accounting();
    let mut inputs = registry(&work, &hir);
    let ticket = inputs.written_ticket(&mut work, head, ALLOWANCE).unwrap();
    let moved = ticket.clone();
    let before = work.balance(&ticket).unwrap();
    for allowance in [0, 1, ALLOWANCE * 2, u64::MAX] {
        assert_eq!(
            inputs.written_ticket(&mut work, head, allowance).unwrap(),
            moved
        );
        work.claim(&moved)
            .unwrap()
            .precharge(AnalysisWork {
                steps: 3,
                inspected_bytes: 0,
                transferred_bytes: 0,
                reserved_bytes: 0,
            })
            .unwrap();
        assert_eq!(work.balance(&moved).unwrap().allowance_ticks(), ALLOWANCE);
    }
    assert!(work.balance(&ticket).unwrap().remaining_ticks() < before.remaining_ticks());
    assert_eq!(inputs.tickets.len(), 1);
    assert_eq!(
        work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
        1
    );
}

#[test]
fn file_qualification_separates_equal_local_occurrences_and_inherited_alias_work() {
    let declaration_hir = parse("type Pair[T] = (T, T)");
    let use_hir = parse("type Input = pkg::Pair[i32]");
    let mut work = accounting();
    let mut declaration_inputs = registry(&work, &declaration_hir);
    let mut use_inputs =
        AnalysisSourceInputRegistry::new(&work, 1, FileId::new(7), &use_hir).unwrap();
    let declaration_site = site(alias_rhs(&declaration_hir, "Pair"));
    let use_site = TypeOccurrenceId::new(FileId::new(7), alias_rhs(&use_hir, "Input"));
    let standalone = declaration_inputs
        .written_ticket(&mut work, declaration_site, ALLOWANCE)
        .unwrap();
    let demanding = use_inputs
        .written_ticket(&mut work, use_site, ALLOWANCE)
        .unwrap();
    assert_ne!(standalone, demanding);
    let standalone_balance = work.balance(&standalone).unwrap();

    for _ in 0..8 {
        assert_eq!(
            declaration_inputs
                .inherited_ticket(&mut work, &demanding)
                .unwrap(),
            demanding
        );
    }
    assert_eq!(work.balance(&standalone).unwrap(), standalone_balance);
    assert_eq!(declaration_inputs.tickets.len(), 1);
    assert_eq!(use_inputs.tickets.len(), 1);
    assert!(matches!(
        declaration_inputs.written_ticket(&mut work, use_site, ALLOWANCE),
        Err(AnalysisSourceInputFailure::NotOriginalType(_))
    ));

    let same_hir = parse("type Other = i32");
    let local = alias_rhs(&same_hir, "Other");
    let mut first = AnalysisSourceInputRegistry::new(&work, 1, FileId::new(9), &same_hir).unwrap();
    let mut second =
        AnalysisSourceInputRegistry::new(&work, 2, FileId::new(10), &same_hir).unwrap();
    let a = first
        .written_ticket(
            &mut work,
            TypeOccurrenceId::new(FileId::new(9), local),
            ALLOWANCE,
        )
        .unwrap();
    let b = second
        .written_ticket(
            &mut work,
            TypeOccurrenceId::new(FileId::new(10), local),
            ALLOWANCE,
        )
        .unwrap();
    assert_ne!(a, b);
}

#[test]
fn wrong_file_nested_and_stale_ordinals_refuse_without_root_or_store_growth() {
    let hir = parse("type Input = Opt[i32]");
    let larger = parse("type Larger = (i32, Bool, Str, f32, Unit)");
    let head = alias_rhs(&hir, "Input");
    let child = hir.type_refs().find(|(id, _)| *id != head).unwrap().0;
    let stale = alias_rhs(&larger, "Larger");
    assert!(stale.get() as usize >= hir.type_refs().len());
    for occurrence in [
        TypeOccurrenceId::new(FileId::new(8), head),
        site(child),
        site(stale),
    ] {
        let mut work = accounting();
        let mut inputs = registry(&work, &hir);
        let (result, reservations) =
            observe(|| inputs.written_ticket(&mut work, occurrence, ALLOWANCE));
        assert!(
            matches!(result, Err(AnalysisSourceInputFailure::NotOriginalType(found)) if found == occurrence)
        );
        assert!(reservations.is_empty());
        assert!(inputs.tickets.is_empty());
        assert!(work.incomplete().is_none());
        assert_eq!(
            work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
            0
        );
    }
}

#[test]
fn invalid_hir_refuses_before_registry_or_root_allocation() {
    let raw = SourceFile::new(FILE, b"type Broken = Fn(i32) MissingArrow".to_vec());
    let parsed = eglur_syntax::parse_validated(raw.validate().unwrap());
    let hir = eglur_hir::lower(&parsed);
    assert!(!hir.invalid_occurrences().is_empty());
    let mut work = accounting();
    assert!(matches!(
        AnalysisSourceInputRegistry::new(&work, 0, FILE, &hir),
        Err(AnalysisSourceInputFailure::InvalidHir(FILE))
    ));
    assert!(work.incomplete().is_none());
    assert_eq!(
        work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
        0
    );
}

#[test]
fn invalid_consumer_and_foreign_registry_refuse_as_internal_custody_failures() {
    let hir = parse("type Input = i32");
    let mut work = accounting();
    assert!(matches!(
        AnalysisSourceInputRegistry::new(&work, 3, FILE, &hir),
        Err(AnalysisSourceInputFailure::Work(
            AnalysisWorkFailure::Claim(AnalysisRootClaimError::UnknownConsumer)
        ))
    ));
    let mut inputs = registry(&work, &hir);
    let mut other = accounting();
    assert!(matches!(
        inputs.written_ticket(&mut other, site(alias_rhs(&hir, "Input")), ALLOWANCE),
        Err(AnalysisSourceInputFailure::Work(
            AnalysisWorkFailure::Claim(AnalysisRootClaimError::ForeignAttempt)
        ))
    ));
    assert!(work.incomplete().is_none());
    assert!(other.incomplete().is_none());
    assert_eq!(
        work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
        0
    );
    assert_eq!(
        other.admit(0, ALLOWANCE).unwrap().identity().requirement(),
        0
    );
}

#[test]
fn cold_refusal_precedes_ticket_publication_and_both_store_reservations() {
    let hir = parse("type Input = i32");
    let mut work = accounting();
    let mut inputs = registry(&work, &hir);
    let head = site(alias_rhs(&hir, "Input"));
    let (result, reservations) = observe(|| inputs.written_ticket(&mut work, head, 0));
    let stop = incomplete(result.unwrap_err());
    assert_eq!(work.incomplete(), Some(stop));
    assert!(matches!(
        stop.reason(),
        AnalysisRootStopReason::Exhausted { .. }
    ));
    assert!(reservations.is_empty());
    assert_eq!(inputs.tickets.capacity(), 0);
    assert!(inputs.tickets.is_empty());
    assert!(matches!(
        inputs.slots[head.type_ref().get() as usize],
        SourceTypeSlot::Original(None)
    ));
    assert_eq!(
        incomplete(
            inputs
                .written_ticket(&mut work, head, u64::MAX)
                .unwrap_err()
        ),
        stop
    );
}

#[test]
fn successful_admission_and_retrieval_have_independent_structural_charge_oracles() {
    let hir = parse("type Input = i32");
    let head = site(alias_rhs(&hir, "Input"));
    let mut baseline = accounting();
    let base_ticket = baseline.admit(0, ALLOWANCE).unwrap();
    let base_charge = baseline.balance(&base_ticket).unwrap().charged_ticks();
    let mut work = accounting();
    let mut inputs = registry(&work, &hir);
    let ticket = inputs.written_ticket(&mut work, head, ALLOWANCE).unwrap();
    let header = size_of::<AnalysisRootTicket>() as u64;
    let slot = size_of::<SourceTypeSlot>() as u64;
    let installation = 2 * 7 + (slot + header + 4 * header + slot) * 2 + 4 * header * 3;
    assert_eq!(
        work.balance(&ticket).unwrap().charged_ticks(),
        base_charge + installation
    );

    let before = work.balance(&ticket).unwrap().charged_ticks();
    inputs.written_ticket(&mut work, head, u64::MAX).unwrap();
    let retrieval = 3 * 7 + (slot + header + header) * 2;
    assert_eq!(
        work.balance(&ticket).unwrap().charged_ticks() - before,
        retrieval
    );
}

#[test]
fn exactly_paid_admission_cannot_replay_for_free_or_renew_its_allowance() {
    let hir = parse("type Input = i32");
    let head = site(alias_rhs(&hir, "Input"));
    let mut generous = accounting();
    let mut probe = registry(&generous, &hir);
    let ticket = probe
        .written_ticket(&mut generous, head, ALLOWANCE)
        .unwrap();
    let admission_cost = generous.balance(&ticket).unwrap().charged_ticks();
    let mut work = accounting();
    let mut inputs = registry(&work, &hir);
    let ticket = inputs
        .written_ticket(&mut work, head, admission_cost)
        .unwrap();
    assert_eq!(work.balance(&ticket).unwrap().remaining_ticks(), 0);
    let stop = incomplete(
        inputs
            .written_ticket(&mut work, head, u64::MAX)
            .unwrap_err(),
    );
    assert_eq!(stop.root(), ticket.identity());
    assert_eq!(
        work.balance(&ticket).unwrap().allowance_ticks(),
        admission_cost
    );
    assert_eq!(inputs.tickets.len(), 1);
}

#[test]
fn pricing_overflow_is_incomplete_without_source_or_ticket_publication() {
    let hir = parse("type Input = i32");
    let mut work =
        AnalysisWorkAccounting::new(AnalysisWorkPricing::for_test(u64::MAX, 1, 1).unwrap(), 1)
            .unwrap();
    let mut inputs = registry(&work, &hir);
    let (result, reservations) =
        observe(|| inputs.written_ticket(&mut work, site(alias_rhs(&hir, "Input")), u64::MAX));
    let stop = incomplete(result.unwrap_err());
    assert!(matches!(
        stop.reason(),
        AnalysisRootStopReason::Unrepresentable(AnalysisWorkOverflow::Price)
    ));
    assert!(reservations.is_empty());
    assert!(inputs.tickets.is_empty());
}

#[test]
fn inherited_loans_do_not_admit_even_when_the_source_has_no_original_types() {
    let hir = parse("");
    let mut work = accounting();
    let ticket = work.admit(2, ALLOWANCE).unwrap();
    let inputs = registry(&work, &hir);
    let before = work.balance(&ticket).unwrap();
    for _ in 0..16 {
        assert_eq!(inputs.inherited_ticket(&mut work, &ticket).unwrap(), ticket);
    }
    assert_eq!(
        work.balance(&ticket).unwrap().allowance_ticks(),
        before.allowance_ticks()
    );
    assert!(work.balance(&ticket).unwrap().remaining_ticks() < before.remaining_ticks());
    assert!(inputs.tickets.is_empty());
    assert_eq!(
        work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
        0
    );
}

#[test]
fn foreign_inherited_ticket_remains_internal_even_after_a_sticky_stop() {
    let hir = parse("type Input = i32");
    let mut work = accounting();
    let mut other = accounting();
    let foreign = other.admit(0, ALLOWANCE).unwrap();
    let inputs = registry(&work, &hir);
    let stopped = work.admit(1, 0).unwrap_err();
    let AnalysisWorkFailure::Incomplete(stop) = stopped else {
        panic!("expected stop")
    };
    assert!(matches!(
        inputs.inherited_ticket(&mut work, &foreign),
        Err(AnalysisSourceInputFailure::Work(
            AnalysisWorkFailure::Claim(AnalysisRootClaimError::ForeignAttempt)
        ))
    ));
    assert_eq!(work.incomplete(), Some(stop));
    assert!(inputs.tickets.is_empty());
}

#[test]
fn sticky_stop_prevents_new_setup_admission_replay_and_inherited_loans() {
    let hir = parse("type First = i32\ntype Second = Bool");
    let mut work = accounting();
    let mut inputs = registry(&work, &hir);
    let first = inputs
        .written_ticket(&mut work, site(alias_rhs(&hir, "First")), ALLOWANCE)
        .unwrap();
    let balance = work.balance(&first).unwrap();
    let stop = incomplete(
        inputs
            .written_ticket(&mut work, site(alias_rhs(&hir, "Second")), 0)
            .unwrap_err(),
    );
    for result in [
        inputs.written_ticket(&mut work, site(alias_rhs(&hir, "First")), u64::MAX),
        inputs.written_ticket(&mut work, site(alias_rhs(&hir, "Second")), u64::MAX),
        inputs.inherited_ticket(&mut work, &first),
    ] {
        assert_eq!(incomplete(result.unwrap_err()), stop);
    }
    let (result, reservations) = observe(|| AnalysisSourceInputRegistry::new(&work, 1, FILE, &hir));
    let Err(error) = result else {
        panic!("setup must stop")
    };
    assert_eq!(incomplete(error), stop);
    assert!(reservations.is_empty());
    assert_eq!(inputs.tickets.len(), 1);
    assert_eq!(work.balance(&first).unwrap(), balance);
}

#[test]
fn many_independent_inputs_keep_dense_tickets_and_pay_the_actual_growth_envelope() {
    let text = (0..257)
        .map(|index| format!("type T{index} = (i32, Bool)\n"))
        .collect::<String>();
    let hir = parse(&text);
    let mut work = accounting();
    let mut inputs = registry(&work, &hir);
    let heads = original_sites(&inputs);
    let (tickets, reservations) = observe(|| {
        heads
            .iter()
            .map(|&head| inputs.written_ticket(&mut work, head, ALLOWANCE).unwrap())
            .collect::<Vec<_>>()
    });
    for (index, ticket) in tickets.iter().enumerate() {
        assert_eq!(ticket.identity().requirement(), index);
        assert_eq!(
            inputs.written_ticket(&mut work, heads[index], 0).unwrap(),
            *ticket
        );
    }
    assert_growth_envelope(
        &reservations,
        tickets.len() * (size_of::<AnalysisRootTicket>() + size_of::<AnalysisRootBalance>()),
    );
    let charged = tickets
        .iter()
        .map(|ticket| work.balance(ticket).unwrap().charged_ticks())
        .sum::<u64>();
    let reserved = reservations
        .iter()
        .filter_map(|record| {
            record
                .requested_capacity
                .map(|capacity| capacity * record.record_bytes)
        })
        .sum::<usize>();
    let relocated = reservations
        .iter()
        .filter(|record| record.requested_capacity.is_some())
        .map(|record| record.len * record.record_bytes)
        .sum::<usize>();
    assert!(charged >= reserved as u64 * 3 + relocated as u64 * 2);
    assert_eq!(
        work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
        heads.len()
    );
}

#[test]
fn storage_history_and_order_do_not_change_the_local_source_admission_charge() {
    let hir = parse("type First = (i32, Bool)\ntype Second = Opt[Str]");
    let heads = [
        site(alias_rhs(&hir, "First")),
        site(alias_rhs(&hir, "Second")),
    ];
    let trace = |warm_count, reverse| {
        let mut work = accounting();
        for _ in 0..warm_count {
            work.admit(0, ALLOWANCE).unwrap();
        }
        let mut inputs = registry(&work, &hir);
        let order = if reverse { [heads[1], heads[0]] } else { heads };
        let tickets = order.map(|head| inputs.written_ticket(&mut work, head, ALLOWANCE).unwrap());
        for &head in &order {
            inputs.written_ticket(&mut work, head, 0).unwrap();
        }
        tickets.map(|ticket| work.balance(&ticket).unwrap().charged_ticks())
    };
    assert_eq!(trace(0, false), trace(17, false));
    assert_eq!(trace(0, false), trace(63, true));
}

#[test]
fn alias_chain_setup_tracks_written_nodes_without_expanding_a_type_dag() {
    let mut text = String::from("type A0 = i32\n");
    for index in 1..=32 {
        text.push_str(&format!(
            "type A{index} = (A{}, A{})\n",
            index - 1,
            index - 1
        ));
    }
    let hir = parse(&text);
    let mut work = accounting();
    let mut inputs = registry(&work, &hir);
    assert_eq!(inputs.slots.len(), hir.type_refs().len());
    let outer = site(alias_rhs(&hir, "A32"));
    let ticket = inputs.written_ticket(&mut work, outer, ALLOWANCE).unwrap();
    for _ in 0..32 {
        assert_eq!(inputs.inherited_ticket(&mut work, &ticket).unwrap(), ticket);
    }
    assert_eq!(inputs.tickets.len(), 1);
    assert_eq!(
        work.admit(0, ALLOWANCE).unwrap().identity().requirement(),
        1
    );
}
