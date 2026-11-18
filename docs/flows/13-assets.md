# Fluxo 13 — Bens e negociação

## Resumo
Como um bem (imóvel, veículo…) é registrado por um **órgão** e passa de dono com pagamento em BRDU: o dinheiro fica em **escrow** durante uma **janela de veto** e só então a propriedade muda.

## Quem participa
- **Autoridades raiz** (gênese, M-de-N): criam as **classes**.
- **Órgão** da classe (M-de-N): registra, veta e põe ou tira gravames.
- **Dono (vendedor)** e **comprador**.
- **Qualquer pessoa:** finaliza depois da janela.

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
```text
cargo dev init --min-window 1 && cargo dev up
cargo dev wallet new alice && cargo dev wallet new bob
cargo dev fund bob 100
cargo dev asset class --id imovel --window 60                 # autoridades de teste criam a classe
cargo dev asset register --class imovel --id casa-1 --owner alice --meta deed   # o órgão de teste registra
cargo dev asset offer --id casa-1 --as alice --to bob --price 40
cargo dev asset accept --id casa-1 --as bob                   # 40 BRDU saem de bob para o escrow; janela começa
cargo dev asset veto --id casa-1                              # (opção A) o órgão veta antes do prazo: dinheiro volta
cargo dev asset finalize --id casa-1                          # (opção B) depois da janela: casa e dinheiro trocam de mão
cargo dev asset lien --id casa-1 --lien hip-1 --ref "contrato 42"   # gravame; unlien remove
cargo dev asset list alice | show casa-1 | deals
```
1. **Classe (5)** e **registro (6)** pelas aprovações do órgão sobre o documento exato.
2. **Oferta (7)**, **aceite (8)**, **cancelamento (9)**: assinados pelo vendedor ou pelo comprador.
3. **Veto (10)** antes do prazo; **finalização (11)** a partir do prazo.
4. **Gravames (12, 13)** pelo órgão; com gravame não se oferta nem se finaliza.
5. **Consulta:** `GET /asset-classes`, `/asset/ID`, `/assets/ENDEREÇO`, `/deals` (nó); o explorador mostra os bens na conta do dono; a carteira mostra um cartão de bens.

## O que é verificado e por quem
O **ledger**, em todo nó: aprovações distintas do órgão sobre o documento (com a **revisão** do bem); só o dono oferece, só o comprador aceita; saldo para o escrow; janela pela hora do **bloco**; gravames; conservação (o escrow é o preço de uma negociação aceita).

## O que pode dar errado
- **Aprovação reaproveitada** (outro bem, comprador, preço, prazo ou revisão antiga): não vale.
- **Veto tarde demais / finalização cedo demais:** `window_closed` / `window_open`.
- **Gravame durante a janela:** a finalização falha e o dinheiro fica no escrow até o órgão remover o gravame ou vetar.
- **Órgão comprometido:** registra bens falsos ou veta à vontade.
- **Saldo insuficiente no aceite:** a transação falha inteira, sem escrow parcial.

## Onde está no código
`crates/brdu-assets` (módulo, documentos, payloads), `crates/brdu-chain/src/chain.rs` (registro do módulo), API em `crates/brdu-node/src/api.rs`, `cargo dev asset` em `tools/brdu-dev/src/asset.rs`, explorador (`view.rs`, `ui/`), carteira (`walletd`, `apps/wallet/ui`).

## Testes que cobrem
`crates/brdu-assets/tests/assets.rs` (10: classe, registro, negociação completa, veto, oferta/cancelamento, aceite sem saldo, gravames, aprovação presa a cada campo, propriedade de conservação) e `tests/oracle.rs` (3: **vetores do BRDC byte a byte**); `tools/brdu-dev/tests/cli.rs` (vida inteira contra nós reais); explorador e carteira (testes de interface e do walletd).

## Limites honestos
O CLI assina com **todas as chaves de teste** do órgão; sem leilão; o índice por dono é uma varredura (O(bens)); o explorador ainda não tem teste de Rust com bens reais (só fixture de interface); diagramas "a gerar".
