use std::time::Duration;

use gh_board::config::types::PrFilter;
use gh_board::engine::{Engine, Event, Request, StubEngine};
use gh_board::types::PullRequest;

fn load_fixture_prs() -> Vec<PullRequest> {
    let json = include_str!("fixtures/stub_prs.json");
    serde_json::from_str(json).expect("valid stub_prs.json fixture")
}

#[test]
fn stub_engine_fetch_prs_returns_fixture_data() {
    let prs = load_fixture_prs();
    assert_eq!(prs.len(), 1, "fixture should have exactly one PR");

    let stub = StubEngine {
        prs: prs.clone(),
        issues: vec![],
        notifications: vec![],
    };

    let handle = stub.start();
    let (tx, rx) = std::sync::mpsc::channel::<Event>();

    let filter = PrFilter {
        title: "All".into(),
        filters: String::new(),
        limit: None,
        host: None,
        layout: None,
    };
    handle.send(Request::FetchPrs {
        filter_idx: 0,
        filter,
        force: false,
        reply_tx: tx,
    });

    let event = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("engine should reply within 2 seconds");

    match event {
        Event::PrsFetched {
            filter_idx,
            prs: fetched_prs,
            ..
        } => {
            assert_eq!(filter_idx, 0);
            assert_eq!(fetched_prs.len(), prs.len());
            assert_eq!(fetched_prs[0].number, 42);
            assert_eq!(fetched_prs[0].title, "Fix: resolve widget layout overflow");
        }
        _other => panic!("expected PrsFetched, got a different event variant"),
    }
}

#[test]
fn stub_engine_mutations_succeed_instantly() {
    let stub = StubEngine {
        prs: vec![],
        issues: vec![],
        notifications: vec![],
    };

    let handle = stub.start();
    let (tx, rx) = std::sync::mpsc::channel::<Event>();

    handle.send(Request::ClosePr {
        owner: "example".into(),
        repo: "repo".into(),
        number: 1,
        reply_tx: tx,
    });

    let event = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("engine should reply within 2 seconds");

    assert!(
        matches!(event, Event::MutationOk { .. }),
        "mutation should return MutationOk"
    );
}

#[test]
fn stub_engine_detail_returns_fetch_error() {
    let stub = StubEngine {
        prs: vec![],
        issues: vec![],
        notifications: vec![],
    };

    let handle = stub.start();
    let (tx, rx) = std::sync::mpsc::channel::<Event>();

    handle.send(Request::FetchPrDetail {
        owner: "example".into(),
        repo: "repo".into(),
        number: 1,
        base_ref: "main".into(),
        head_repo_owner: None,
        head_ref: "feature".into(),
        force: false,
        reply_tx: tx,
    });

    let event = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("engine should reply within 2 seconds");

    assert!(
        matches!(event, Event::FetchError { .. }),
        "detail fetch on stub should return FetchError"
    );
}

#[test]
fn stub_engine_fetch_run_by_id_returns_not_found() {
    let stub = StubEngine {
        prs: vec![],
        issues: vec![],
        notifications: vec![],
    };

    let handle = stub.start();
    let (tx, rx) = std::sync::mpsc::channel::<Event>();

    handle.send(Request::FetchRunById {
        owner: "example".into(),
        repo: "repo".into(),
        run_id: 999,
        host: None,
        reply_tx: tx,
    });

    let event = rx
        .recv_timeout(Duration::from_secs(2))
        .expect("engine should reply within 2 seconds");

    assert!(
        matches!(
            event,
            Event::SingleRunFetched {
                run_id: 999,
                run: None,
                ..
            }
        ),
        "FetchRunById on stub should return SingleRunFetched with run: None"
    );
}
