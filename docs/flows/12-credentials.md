# Fluxo 12 — Credenciais

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
Como um **órgão** declara algo sobre uma pessoa na rede e como essa declaração passa por uma **janela de contestação** antes de valer. A rede guarda quem declarou, quando e se ainda vale; não decide se é verdade.

## Quem participa
- **Autoridades raiz** (gênese, M-de-N): criam os **tipos**.
- **Órgão** do tipo (M-de-N): emite, veta e revoga.
- **Titular:** pode contestar enquanto a janela está aberta.
- **Qualquer pessoa:** ativa depois da janela; consulta.

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
```text
cargo dev init --min-window 1 && cargo dev up
cargo dev wallet new alice
cargo dev cred type --id cnh --contest 2                       # autoridades de teste criam o tipo
cargo dev cred issue --type cnh --id cnh-1 --to alice --data category=B   # o órgão de teste emite (pendente)
cargo dev cred show cnh-1                                      # [pending]
cargo dev cred activate --id cnh-1                             # depois da janela: [active], valid: true
cargo dev cred list alice                                      # credenciais do titular
cargo dev cred revoke --id cnh-1 --reason fraud                # o órgão revoga: [revoked]
```
1. **Tipo (14):** as autoridades assinam o documento exato (rede, id, órgão, limite, janela, validade, privacidade, referência).
2. **Emissão (15):** o órgão assina `{rede, tipo, id, titular, dados, referência}`; a credencial nasce **pendente** com `prazo = hora do bloco + janela`.
3. **Contestar (16):** só o titular, antes do prazo. **Vetar (17):** o órgão, antes do prazo. Os dois são **finais**.
4. **Ativar (18):** depois do prazo, qualquer um; se o tipo "revoga a referência" (divórcio), a credencial referenciada (casamento) é revogada.
5. **Revogar (19):** o órgão, pendente ou ativa; final.
6. **Consulta:** `GET /credential-types`, `/credential/ID`, `/credentials/ENDEREÇO` (nó); o **explorador** mostra as credenciais na conta do titular (e a emissão aparece nas movimentações dele); a **carteira** mostra um cartão de credenciais no início.

## O que é verificado e por quem
O **ledger**, em todo nó: autoridades (para tipos) e órgão (para o resto) com **mais que M aprovações distintas** sobre o documento exato; `cred_id` livre; formato dos dados do tipo; referência ativa, do tipo e do titular certos; janela pela hora do **bloco**; quem pode contestar.

## O que pode dar errado
- **Aprovação reaproveitada** (outro id, titular, dado, rede, motivo ou hora de emissão): não vale (documento diferente).
- **Ativar antes do prazo / contestar depois:** recusado. **Id repetido:** `credential_exists`, mesmo se a anterior foi contestada, vetada ou revogada.
- **Órgão comprometido:** emite ou revoga indevidamente (limite honesto da rede).
- **Dados privados:** só o compromisso está na rede; quem perde o sal não consegue revelar.

## Onde está no código
`crates/brdu-credentials` (módulo, documentos, payloads), `crates/brdu-ledger/src/approvals.rs`, regras em `crates/brdu-ledger/src/rules.rs` e na gênese (`crates/brdu-chain`), API em `crates/brdu-node/src/api.rs`, `cargo dev cred` em `tools/brdu-dev/src/cred.rs`, explorador (`view.rs`, `ui/`), carteira (`walletd`, `apps/wallet/ui`).

## Testes que cobrem
`crates/brdu-credentials/tests/credentials.rs` (21: tipo com 2 de 3 autoridades, malformados, taxa, emissão, privadas, aprovação presa a cada campo, janela, contestar, vetar, ativar, **hora do bloco**, validade no limite exato, revogação, casamento e divórcio, id queimado, payloads malformados, propriedade de transições legais) e `tests/oracle.rs` (3: **vetores do BRDC byte a byte**, separação de domínio); `crates/brdu-node/tests/cluster.rs` (vida inteira em 4 validadores reais); `tools/brdu-dev/tests/cli.rs` (`cred`); `tools/brdu-explorer/tests/explorer.rs` e `crates/brdu-walletd/tests/wallet.rs` (credenciais na conta e na carteira); testes da interface.

## Limites honestos
O CLI assina com **todas as chaves de teste** do órgão (propostas coletivas por membro, ADR-0024 do BRDC, são planejadas); sem credencial anônima/ZK; o índice por titular é uma varredura (O(credenciais)); diagramas "a gerar".
