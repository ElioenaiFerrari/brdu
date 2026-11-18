//! Laboratório Docker: gera `compose/` (chaves, gênese e `docker-compose.yml` com limites de CPU e memória) e
//! embrulha o `docker compose`. Uma imagem só (`brdu-lab:dev`) para todos os nós.

use std::fs;
use std::path::Path;
use std::process::Command;

use crate::net;

const IMAGE: &str = "brdu-lab:dev";

fn docker(args: &[&str]) -> Result<bool, String> {
    Command::new("docker")
        .args(args)
        .status()
        .map(|s| s.success())
        .map_err(|e| format!("docker: {e}"))
}

fn compose(dir: &Path, args: &[&str]) -> Result<bool, String> {
    let file = dir.join("docker-compose.yml");
    let mut full = vec![
        "compose",
        "-f",
        file.to_str().unwrap_or("compose/docker-compose.yml"),
    ];
    full.extend_from_slice(args);
    docker(&full)
}

/// `compose init`: chaves, gênese e o arquivo do compose.
pub fn init(
    dir: &Path,
    nodes: usize,
    spare: usize,
    cpus: &str,
    mem_mib: u64,
    min_block_ms: u64,
) -> Result<(), String> {
    let spec = net::Spec {
        chain: "brdu-lab",
        nodes,
        spare,
        issuers: 3,
        threshold: 2,
    };
    net::init(
        dir,
        &spec,
        &|i| format!("v{i}:7000"),
        &|i| format!("127.0.0.1:{}", 7300 + i),
        &|i| format!("127.0.0.1:{}", 8200 + i),
    )?;
    let total = nodes + spare;
    let mut y = String::new();
    y.push_str("# GERADO por `cargo dev compose init`: não edite à mão (rode `compose clean` e `compose init` de novo).\n");
    y.push_str("name: brdu-lab\n\nx-node: &node\n");
    y.push_str(&format!("  image: {IMAGE}\n  user: \"0:0\"\n  restart: \"no\"\n  cpus: {cpus}\n  mem_limit: {mem_mib}m\n  memswap_limit: {mem_mib}m\n  pids_limit: 256\n"));
    y.push_str("  healthcheck:\n    test: [\"CMD\", \"brdu-node\", \"health\", \"--api\", \"127.0.0.1:8080\"]\n    interval: 2s\n    timeout: 3s\n    retries: 15\n    start_period: 3s\n\nservices:\n");
    for i in 1..=total {
        y.push_str(&format!("  v{i}:\n    <<: *node\n    command:\n"));
        for a in [
            "run",
            "--genesis",
            "/lab/genesis.hex",
            "--key",
            &format!("/lab/keys/v{i}.key"),
            "--data",
            "/data",
            "--listen",
            "0.0.0.0:7000",
            "--api",
            "0.0.0.0:8080",
            "--min-block-ms",
            &min_block_ms.to_string(),
        ] {
            y.push_str(&format!("      - \"{a}\"\n"));
        }
        for j in 1..i {
            y.push_str(&format!("      - \"--peer\"\n      - \"v{j}:7000\"\n"));
        }
        y.push_str(&format!(
            "    volumes:\n      - ./genesis.hex:/lab/genesis.hex:ro\n      - ./keys/v{i}.key:/lab/keys/v{i}.key:ro\n      - data-v{i}:/data\n    ports:\n      - \"127.0.0.1:{}:8080\"\n      - \"127.0.0.1:{}:7000\"\n",
            8200 + i,
            7300 + i
        ));
    }
    y.push_str("\nvolumes:\n");
    for i in 1..=total {
        y.push_str(&format!("  data-v{i}:\n"));
    }
    fs::write(dir.join("docker-compose.yml"), y).map_err(|e| e.to_string())?;
    println!(
        "laboratório criado em {}: {nodes} validadores, {cpus} CPU e {mem_mib} MiB cada",
        dir.display()
    );
    println!("próximo: cargo dev compose up   (a primeira vez compila a imagem, alguns minutos)");
    Ok(())
}

fn ensure_image() -> Result<(), String> {
    if Command::new("docker")
        .args(["image", "inspect", IMAGE])
        .output()
        .is_ok_and(|o| o.status.success())
    {
        return Ok(());
    }
    build()
}

/// `compose build`: constrói a imagem única.
pub fn build() -> Result<(), String> {
    println!("construindo {IMAGE} (cache de dependências no BuildKit)…");
    if docker(&["build", "-f", "docker/Dockerfile", "-t", IMAGE, "."])? {
        Ok(())
    } else {
        Err("docker build falhou".into())
    }
}

/// `compose up`: imagem, contêineres e espera ficarem saudáveis.
pub fn up(dir: &Path) -> Result<(), String> {
    ensure_image()?;
    if !compose(dir, &["up", "-d", "--wait", "--wait-timeout", "90"])? {
        return Err("docker compose up falhou (veja `cargo dev compose logs`)".into());
    }
    println!(
        "no ar. Consulte com: cargo dev --dir {} status",
        dir.display()
    );
    Ok(())
}

/// Os demais comandos.
pub fn run(dir: &Path, sub: &str, rest: &[String]) -> Result<(), String> {
    let svc = |i: &str| format!("v{i}");
    let ok = match sub {
        "build" => return build(),
        "up" => return up(dir),
        "down" => compose(dir, &["down"])?,
        "ps" => compose(dir, &["ps"])?,
        "logs" => {
            let mut a = vec!["logs", "--tail", "60"];
            let s = rest.first().map(|i| svc(i));
            if let Some(s) = &s {
                a.push(s);
            }
            compose(dir, &a)?
        }
        "pause" | "unpause" | "kill" | "start" | "stop" | "restart" => {
            let i = rest
                .first()
                .ok_or_else(|| format!("compose {sub} NÚMERO_DO_NÓ"))?;
            compose(dir, &[sub, &svc(i)])?
        }
        "stats" => docker(&["stats", "--no-stream"])?,
        "clean" => {
            // derruba SÓ este laboratório (projeto brdu-lab, com os seus volumes) e apaga os arquivos gerados
            let _ = compose(dir, &["down", "--volumes", "--remove-orphans"]);
            for sub in ["keys", "wallets"] {
                let _ = fs::remove_dir_all(dir.join(sub));
            }
            for f in ["devnet.json", "genesis.hex", "docker-compose.yml"] {
                let _ = fs::remove_file(dir.join(f));
            }
            let _ = fs::remove_dir(dir);
            println!(
                "laboratório em {} apagado (a imagem {IMAGE} fica; `docker rmi {IMAGE}` a remove)",
                dir.display()
            );
            true
        }
        other => {
            return Err(format!(
                "compose {other}: use init | build | up | down | ps | logs [N] | pause N | unpause N | kill N | start N | stop N | restart N | stats | clean"
            ));
        }
    };
    if ok {
        Ok(())
    } else {
        Err("o docker retornou erro".into())
    }
}
