use eglur_source::{FileId, SourceFile};
use eglur_syntax::{FrontendOutcome, SyntaxElement, SyntaxKind, parse};

#[test]
fn parsed_trees_can_move_between_threads() {
    let source = SourceFile::new(FileId::new(3), b"fn f() -> Unit = ()\n".to_vec());
    let parsed = parse(&source);
    std::thread::spawn(move || {
        let FrontendOutcome::Parsed(parsed) = parsed else {
            panic!("UTF-8")
        };
        assert!(parsed.is_success());
        assert_eq!(parsed.syntax().text(), "fn f() -> Unit = ()\n");
    })
    .join()
    .unwrap();
}

#[test]
fn deep_trees_drop_after_success_and_recovery_on_the_default_stack() {
    for suffix in ["", ".", ".then("] {
        let text = format!(
            "fn f() -> Unit = a{}{suffix}\nfn next() -> Unit = ()\n",
            ".member()".repeat(4000)
        );
        let source = SourceFile::new(FileId::new(1), text.clone().into_bytes());
        let FrontendOutcome::Parsed(parsed) = parse(&source) else {
            panic!("UTF-8")
        };
        assert_eq!(parsed.is_success(), suffix.is_empty());
        assert_eq!(parsed.syntax().text(), text);
        drop(parsed);
    }
}

#[test]
fn detached_nodes_and_tokens_keep_the_tree_until_the_last_view_drops() {
    let text = format!("fn f() -> Unit = a{}\n", ".member()".repeat(4000));
    let source = SourceFile::new(FileId::new(2), text.clone().into_bytes());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("UTF-8")
    };
    assert!(parsed.is_success());
    let clone = parsed.clone();
    let mut node = parsed.syntax();
    loop {
        let next = node.children().next();
        match next {
            Some(next) => node = next,
            None => break,
        }
    }
    let token = node
        .children_with_tokens()
        .find_map(|element| match element {
            SyntaxElement::Token(token) if !token.is_missing() => Some(token),
            _ => None,
        })
        .expect("leaf has a written token");
    let spelling = token.text().to_owned();
    let span = token.span();
    drop(parsed);
    drop(node);
    assert_eq!(clone.syntax().text(), text);
    drop(clone);
    assert_eq!(token.text(), spelling);
    assert_eq!(token.span(), span);
    drop(token);
}

#[test]
fn the_deepest_receiver_can_outlive_the_parse_and_other_views() {
    let text = format!("fn f() -> Unit = a{}\n", ".member()".repeat(4000));
    let source = SourceFile::new(FileId::new(4), text.into_bytes());
    let FrontendOutcome::Parsed(parsed) = parse(&source) else {
        panic!("UTF-8")
    };
    let mut pending = vec![parsed.syntax()];
    let mut receiver = None;
    while let Some(node) = pending.pop() {
        if node.kind() == SyntaxKind::NameExpr && node.text().trim() == "a" {
            receiver = Some(node.clone());
        }
        pending.extend(node.children());
    }
    let receiver = receiver.expect("deepest receiver remains available");
    let clone = receiver.clone();
    drop(parsed);
    drop(receiver);
    assert_eq!(clone.text().trim(), "a");
    drop(clone);
}
