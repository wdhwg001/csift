use super::*;

fn parse(line: &str) -> Record {
    serde_json::from_str(line).expect("valid record")
}

/// A spawn lookup stub: `toolu_spawn` spawned `child-abc`; the teammate name `VSRepro`
/// resolves to its name-embedded id. Everything else is unknown (graceful degrade path).
struct FakeSpawn;
impl SpawnLookup for FakeSpawn {
    fn child_for_spawn_tool_use_id(&self, id: &str) -> Option<String> {
        (id == "toolu_spawn").then(|| "child-abc".to_string())
    }
    fn child_for_spawn_name(&self, name: &str) -> Option<String> {
        (name == "VSRepro").then(|| "aVSRepro-deadbeef".to_string())
    }
}

mod part01;
mod part02;
mod part03;
mod part04;
mod part05;
mod part06;
mod part07;
mod part08;
