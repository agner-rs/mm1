use std::fmt;
use std::time::Duration;

use test_case::test_case;

use super::*;

enum Action<K = usize> {
    Add(K),
    Rm(K),
    Decide,
    Started(K, Address),
    Exited(Address),
    Delay(Duration),
}

use Action::*;

#[test_case(
    Default::default(),
    None::<Action<usize>>
    ; "empty"
)]
#[test_case(
    Default::default(),
    [
        Add("one"),
        Rm("one"),
        Decide,
    ]
    ; "add, remove, decide later"
)]
#[test_case(
    Default::default(),
    [
        Add("one"),
        Decide,
        Started("one", Address::from_u64(1)),
        Rm("one"),
        Decide,
        Exited(Address::from_u64(1)),
        Decide,
    ]
    ; "add, started, remove"
)]
#[test_case(
    Default::default(),
    [
        Add("one"),
        Add("two"),
        Decide,
        Started("one", Address::from_u64(1)),
        Decide,
        Started("two", Address::from_u64(2)),
        Decide,
    ]
    ; "add two, started, remove first"
)]
#[test_case(
    Default::default(),
    [
        Add("one"),
        Decide,
        Started("one", Address::from_u64(1)),
        Decide,
        Exited(Address::from_u64(1)),
        Decide,
    ]
    ; "add, started, exited, max restart intensity reached"
)]
#[test_case(
    Default::default(),
    [
        Add("one"),
        Add("two"),
        Add("three"),
        Decide,
        Started("one", Address::from_u64(1)),
        Started("two", Address::from_u64(2)),
        Started("three", Address::from_u64(3)),
        Decide,
        Exited(Address::from_u64(1)),
        Decide,
        Exited(Address::from_u64(2)),
        Exited(Address::from_u64(3)),
        Decide,
    ]
    ; "add several, started, exited, max restart intensity reached"
)]
#[test_case(
    RestartIntensity { max_restarts: 1, within: Duration::from_secs(30) },
    [
        Add("one"),
        Decide,
        Started("one", Address::from_u64(1)),
        Decide,
        Delay(Duration::from_secs(40)),
        Exited(Address::from_u64(1)),
        Decide,
        Started("one", Address::from_u64(2)),
    ]
    ; "add, started, exited, max restart intensity not reached"
)]
#[test_case(
    RestartIntensity { max_restarts: 1, within: Duration::from_secs(30) },
    [
        Add("one"),
        Decide,
        Started("one", Address::from_u64(1)),
        Decide,
        Started("one", Address::from_u64(2)),
        Decide,
    ]
    ; "an orphan"
)]
#[tokio::test]
async fn run<K>(restart_intensity: RestartIntensity, script: impl IntoIterator<Item = Action<K>>)
where
    OneForOne<K>: RestartStrategy<K>,
    K: fmt::Display + Clone + Eq,
{
    mm1_logger::init(&logger_config()).unwrap();
    tokio::time::pause();

    let snapshot_name = std::thread::current()
        .name()
        .expect("empty thread name")
        .to_owned();
    let mut report = vec![];

    report.push(restart_intensity.to_string());
    let mut decider = OneForOne::<K>::new(restart_intensity).decider();

    for action in script {
        report.push(action.to_string());
        let result = match action {
            Delay(d) => {
                tokio::time::sleep(d).await;
                Ok(())
            },
            Started(key, addr) => {
                decider.started(&key, addr);
                Ok(())
            },
            Exited(addr) => {
                decider.exited(addr);
                Ok(())
            },
            Add(key) => decider.add(key),
            Rm(key) => {
                decider.rm(&key);
                Ok(())
            },
            Decide => {
                let mut noop_counter = 0;
                loop {
                    match decider.next_action(tokio::time::Instant::now()) {
                        Err(reason) => break Err(reason),
                        Ok(None) => break Ok(()),
                        Ok(Some(super::Action::Noop)) => {
                            noop_counter += 1;
                            if noop_counter > 10 {
                                report.push("too many Noops: giving up...".into());
                                break Ok(())
                            }
                        },
                        Ok(Some(action)) => report.push(format!(">>> {}", action)),
                    }
                }
            },
        };
        if let Err(reason) = result {
            report.push(format!("!!! ERROR: {}", reason));
            break
        }
    }

    insta::assert_yaml_snapshot!(snapshot_name, report);
}

impl<K> fmt::Display for Action<K>
where
    K: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Add(k) => write!(f, "ADD     [{}]", k),
            Self::Rm(k) => write!(f, "RM      [{}]", k),
            Self::Decide => write!(f, "DECIDE"),
            Self::Started(k, a) => write!(f, "STARTED [{}] / {}", k, a),
            Self::Exited(a) => write!(f, "EXITED  {}", a),
            Self::Delay(d) => write!(f, "DELAY {:?}", d),
        }
    }
}

fn logger_config() -> mm1_logger::LoggingConfig {
    use mm1_logger::*;

    LoggingConfig {
        min_log_level:     Level::TRACE,
        log_target_filter: vec![
            // "mm1_node::runtime::local_system::protocol_actor=trace".parse().unwrap(),
            "mm1_sup::*=DEBUG".parse().unwrap(),
            "*=INFO".parse().unwrap(),
        ],
    }
}
