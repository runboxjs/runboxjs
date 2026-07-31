use criterion::{Criterion, black_box, criterion_group, criterion_main};
use runbox::process::ProcessManager;
use runbox::runtime::Runtime;
use runbox::runtime::git::GitRuntime;
use runbox::runtime::js_engine::strip_typescript;
use runbox::shell::Command;
use runbox::vfs::Vfs;

fn bench_command_parse(c: &mut Criterion) {
    c.bench_function("command_parse_complex", |b| {
        b.iter(|| {
            let cmd = Command::parse(black_box(
                "NODE_ENV=production API_KEY=test bun run src/index.ts --watch --hot",
            ))
            .expect("command parse should succeed");
            black_box(cmd.program);
        });
    });
}

fn bench_strip_typescript(c: &mut Criterion) {
    let ts = r#"
interface User {
    id: number;
    name: string;
}

type Status = "active" | "inactive";
import type { Config } from "./types";

export function mapUsers(input: Array<User>): Array<{ id: number; label: string }> {
    return input.map((u: User) => ({
        id: u.id,
        label: `${u.name}:${u.id}`,
    }));
}
"#;

    c.bench_function("strip_typescript_medium_file", |b| {
        b.iter(|| {
            let js = strip_typescript(black_box(ts)).expect("ts stripping should succeed");
            black_box(js.len());
        });
    });
}

fn bench_vfs_read_write(c: &mut Criterion) {
    c.bench_function("vfs_write_read_100_files", |b| {
        b.iter(|| {
            let mut vfs = Vfs::new();
            for i in 0..100 {
                let path = format!("/src/file_{i}.ts");
                let content = format!("export const value_{i}: number = {i};");
                vfs.write(&path, content.into_bytes())
                    .expect("write should succeed");
            }

            let mut total = 0usize;
            for i in 0..100 {
                let path = format!("/src/file_{i}.ts");
                total += vfs.read(&path).expect("read should succeed").len();
            }
            black_box(total);
        });
    });
}

fn bench_git_status_small_repo(c: &mut Criterion) {
    c.bench_function("git_status_small_repo", |b| {
        b.iter(|| {
            let mut vfs = Vfs::new();
            let mut pm = ProcessManager::new();
            let rt = GitRuntime;

            rt.exec(
                &Command::parse("git init").expect("parse git init"),
                &mut vfs,
                &mut pm,
            )
            .expect("git init");
            vfs.write("/src/main.ts", b"console.log('hi')".to_vec())
                .expect("write source");
            rt.exec(
                &Command::parse("git add .").expect("parse git add"),
                &mut vfs,
                &mut pm,
            )
            .expect("git add");
            rt.exec(
                &Command::parse(r#"git commit -m "bench""#).expect("parse git commit"),
                &mut vfs,
                &mut pm,
            )
            .expect("git commit");
            let out = rt
                .exec(
                    &Command::parse("git status").expect("parse git status"),
                    &mut vfs,
                    &mut pm,
                )
                .expect("git status");
            black_box(out.stdout.len());
        });
    });
}

criterion_group!(
    core_benchmarks,
    bench_command_parse,
    bench_strip_typescript,
    bench_vfs_read_write,
    bench_git_status_small_repo
);
criterion_main!(core_benchmarks);
