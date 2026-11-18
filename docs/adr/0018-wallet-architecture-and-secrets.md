# ADR-0018 — Arquitetura da carteira e tratamento de segredos

**Status:** Aceito (2026-11-18) · **Origem:** BRDC (cofre, passphrase nunca guardada, confirmação do endereço) e a carteira do Teko

## Decisão
- **Um serviço, duas portas.** A lógica da carteira vive em `brdu-walletd` (Rust) e é exposta por uma única interface JSON (`Wallet::call`). O **app desktop** (Tauri v2) a chama por **um comando** (`rpc`) dentro do próprio processo; o **daemon web** (`brdu-web`) a expõe em `POST /rpc` para o navegador e o Docker. A interface (`apps/wallet/ui`) é **a mesma** nas duas.
- **A chave nunca vai para a tela.** O webview/navegador só recebe dados públicos (endereço, saldo, prévia do envio) e as **24 palavras uma única vez**, na criação (ou ao reexibir, com a senha de novo). Senha e passphrase só entram e nunca voltam; os campos são limpos depois de usados.
- **Cofre (`brdu-vault`):** a frase é selada com **ChaCha20-Poly1305** sob uma chave derivada da senha por **Argon2id** (64 MiB, 3 passes por padrão; parâmetros gravados no arquivo e limitados ao abrir). Cabeçalho (versão, KDF, sal, **endereço**) é dado autenticado: adulterar qualquer campo falha. Arquivo com permissão 0600; nunca é sobrescrito por `create`/`import`.
- **A passphrase (25ª palavra) nunca é guardada.** O cofre guarda só o **endereço** da conta; ao desbloquear, o endereço derivado tem de bater, senão `wrong_passphrase`.
- **Importar exige confirmar o endereço:** `preview_import` mostra o endereço sem guardar nada; `import` só vale com o endereço confirmado (`address_mismatch` se diferir).
- **Envio em dois passos:** `prepare_send` valida endereço (checksum), valor (formato brasileiro, sem ponto flutuante), saldo para **valor + taxa** e devolve a prévia (taxa, total, saldo depois); `confirm_send` assina e envia. A prévia é **de uso único**, expira em 5 minutos e some ao bloquear.
- **Defesas:** bloqueio automático por inatividade (5 min); **limite de tentativas de senha** (5 erros e começa um bloqueio que dobra, até 60 s; vale também para exportar a frase e trocar a senha); exportar a frase exige a senha **de novo**.
- **Remover a carteira do aparelho** (`forget`): sem senha (ela pode ter sido esquecida), mas com confirmação explícita; o cofre é **movido de lado**, nunca apagado. Só a frase de recuperação recupera o saldo depois.
- **Daemon web:** escuta só em loopback (recusa outro endereço sem `--allow-remote`); confere `Host` e `Origin` (rebinding de DNS e requisições de outros sites); `/rpc` só aceita `application/json`, com limite de corpo; CSP estrita; sem CORS; respostas sem cache.

## Limites honestos
- **No navegador/daemon, as chaves vivem no processo do daemon** (chaves de **teste**). Não é uma carteira não custodial de verdade: isso exigiria assinar no próprio navegador (WASM), planejado. No app desktop o processo é o do usuário, sem rede de entrada.
- Quem tem a senha e o arquivo do cofre abre a carteira; o Argon2id só encarece a força bruta. Sem auditoria.
- O "confirmar no dispositivo" do projeto Teko não existe aqui: a confirmação é a tela de revisão.
