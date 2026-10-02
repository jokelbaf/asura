use zbus::blocking::Connection;
use zbus::proxy;
use zbus::zvariant::{OwnedObjectPath, Value};

use crate::{Cgroup, Error};

#[proxy(
    interface = "org.freedesktop.systemd1.Manager",
    default_service = "org.freedesktop.systemd1",
    default_path = "/org/freedesktop/systemd1"
)]
trait Manager {
    fn subscribe(&self) -> zbus::Result<()>;

    fn start_transient_unit(
        &self,
        name: &str,
        mode: &str,
        properties: &[(&str, Value<'_>)],
        auxiliary: &[(&str, &[(&str, Value<'_>)])],
    ) -> zbus::Result<OwnedObjectPath>;

    #[zbus(signal)]
    fn job_removed(
        &self,
        id: u32,
        job: OwnedObjectPath,
        unit: String,
        result: String,
    ) -> zbus::Result<()>;
}

#[derive(Clone, Debug)]
pub struct Scope {
    pub unit: String,
    pub cgroup: Cgroup,
}

impl Scope {
    pub fn create(name: &str, processes: &[u32]) -> Result<Self, Error> {
        let leader = processes[0];
        let unit = format!("asura-{}-{leader}.scope", sanitize(name));

        let properties = [
            ("PIDs", Value::from(processes.to_vec())),
            (
                "Description",
                Value::from(format!("Asura capture of {name}")),
            ),
            ("CollectMode", Value::from("inactive-or-failed")),
        ];

        start(&unit, &properties)?;

        Ok(Self {
            unit,
            cgroup: Cgroup::of(leader)?,
        })
    }
}

pub fn start_service(
    name: &str,
    program: &str,
    arguments: &[String],
    environment: &[String],
) -> Result<String, Error> {
    let unit = format!("{name}-{}.service", std::process::id());

    let command_line: Vec<String> = std::iter::once(program.to_owned())
        .chain(arguments.iter().cloned())
        .collect();

    let properties = [
        ("Description", Value::from(name.to_owned())),
        (
            "ExecStart",
            Value::from(vec![(program.to_owned(), command_line, false)]),
        ),
        ("Environment", Value::from(environment.to_vec())),
        ("CollectMode", Value::from("inactive-or-failed")),
        ("Type", Value::from("exec")),
    ];

    start(&unit, &properties)?;
    Ok(unit)
}

fn start(unit: &str, properties: &[(&str, Value<'_>)]) -> Result<(), Error> {
    let connection = Connection::session()?;
    let manager = ManagerProxyBlocking::new(&connection)?;
    let mut removals = manager.receive_job_removed()?;
    manager.subscribe()?;

    let job = manager.start_transient_unit(unit, "fail", properties, &[])?;

    let result = removals
        .find_map(|signal| {
            let arguments = signal.args().ok()?;
            (arguments.job == job).then(|| arguments.result.clone())
        })
        .unwrap_or_default();

    if result != "done" {
        return Err(Error::Scope {
            unit: unit.to_owned(),
            result,
        });
    }

    Ok(())
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '.' => character,
            _ => '_',
        })
        .collect()
}
