use runbox::runtime::js_engine::strip_typescript;
use runbox::shell::{Command, RuntimeTarget};
use runbox::vfs::Vfs;

#[test]
fn command_parse_and_detect_runtime() {
    let cmd = Command::parse("NODE_ENV=production npm run start").expect("command should parse");
    assert_eq!(cmd.program, "npm");
    assert_eq!(RuntimeTarget::detect(&cmd), RuntimeTarget::Npm);
    assert_eq!(cmd.env.len(), 1);
}

#[test]
fn vfs_roundtrip_and_listing() {
    let mut vfs = Vfs::new();
    vfs.write("/src/main.js", b"console.log('ok')".to_vec())
        .expect("write should succeed");
    vfs.write("/src/lib/util.js", b"module.exports = 1".to_vec())
        .expect("write should succeed");

    let file = vfs.read("/src/main.js").expect("read should succeed");
    assert_eq!(file, b"console.log('ok')");

    let src_entries = vfs.list("/src").expect("list should succeed");
    assert!(src_entries.iter().any(|e| e == "main.js"));
    assert!(src_entries.iter().any(|e| e == "lib"));
}

#[test]
fn strip_typescript_removes_type_constructs() {
    let ts = r#"
interface Profile { name: string }
type Id = string;
import type { Something } from "./types";
const value: number = 42;
"#;
    let js = strip_typescript(ts).expect("strip should succeed");
    assert!(!js.contains("interface Profile"));
    assert!(!js.contains("type Id"));
    assert!(!js.contains("import type"));
    assert!(js.contains("const value"));
}
