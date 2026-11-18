# Fluxo 11 — A carteira

> Diagrama: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Resumo
Criar ou importar uma carteira, guardar a frase de recuperação num cofre criptografado, ver o saldo e a cobertura da reserva, receber, enviar com prévia de taxa e total, e consultar o histórico. A mesma interface roda no **app desktop (Tauri v2)** e no **navegador** (daemon local `brdu-web`). A chave fica no processo Rust e nunca vai para a tela.

## Quem participa
- **Você** (e a interface em pt-BR, tema do BRDC).
- **Serviço da carteira** (`brdu-walletd`): cofre, desbloqueio, assinatura.
- **Nó** (`/account`, `/supply`, `/fees`, `POST /tx`) e **explorador** (histórico).

## Diagramas
Diagrama deste fluxo: **a gerar** (diagramas adiados a pedido do dono; foco na implementação).

## Passo a passo
```text
cargo dev init && cargo dev up && cargo dev explorer      # rede local e explorador
cargo dev wallet-web                                       # carteira no navegador: http://127.0.0.1:8546
cd apps/wallet/src-tauri && cargo tauri dev                # ou: o app desktop
```
1. **Criar:** senha (mínimo 8) e, opcionalmente, passphrase. O serviço gera 24 palavras, sela o cofre, mostra as palavras **uma vez** e só avança depois de você marcar que anotou.
2. **Importar:** cole 12 ou 24 palavras (e a passphrase, se houver) → a tela mostra o **endereço** que elas abrem → só depois de confirmar ("Sim, é este") pede a senha do aparelho.
3. **Desbloquear:** senha (e passphrase, se usa). Errar 5 vezes começa um bloqueio temporário.
4. **Início:** saldo (decimais apagados), endereço para copiar, altura, oferta e a cobertura da reserva; atualiza sozinho.
5. **Enviar:** endereço (o checksum denuncia erro de digitação) e valor pelo teclado numérico → **Revisar** (valor, taxa, total, saldo depois; aviso se for para si mesmo) → **Confirmar e enviar**. A prévia é de uso único.
6. **Receber:** o endereço, para copiar. **Histórico:** movimentações do explorador. **Ajustes:** reexibir a frase (pede a senha de novo), trocar a senha, bloquear.
7. A carteira **bloqueia sozinha** após 5 minutos parada e volta à tela de senha.
8. **Esqueci a senha / quero outra carteira:** na tela de senha, "Esqueci a senha ou quero outra carteira" → digite `APAGAR` → a carteira é **removida deste aparelho** (o arquivo do cofre é guardado de lado, `vault.json.removed-<hora>`, nunca apagado) → a tela volta ao início: **criar** uma nova ou **importar** a frase de 24 palavras. **Só a frase recupera o dinheiro.**

> **Onde fica o cofre:** é um **arquivo** (`net/wallet/vault.json` no daemon web; a pasta de dados do app no desktop), **não** cookies nem dados do navegador: limpar o navegador não apaga a carteira (diferente do BRDC, que guardava o cofre no navegador). `cargo dev reset` apaga `net/wallet`.

> **Pelo Docker:** `cargo dev compose up` sobe a carteira web junto com a rede em `http://127.0.0.1:8547` (cofre no volume `data-wallet`; `cargo dev compose clean` o apaga junto com o laboratório).

## O que é verificado e por quem
O serviço valida endereço, valor, saldo e taxa **antes** de assinar; a **rede** valida de novo (assinatura, nonce, taxa mínima, saldo). O cofre autentica o cabeçalho (inclusive o endereço).

## O que pode dar errado
- **Frase perdida = carteira perdida** (não há recuperação; sem congelamento nem estorno).
- **Senha esquecida:** só a frase recupera: remova a carteira do aparelho (acima) e importe a frase. **Passphrase esquecida:** o endereço muda e a carteira não abre.
- **Rede fora do ar:** a tela avisa e tenta de novo; nada é enviado.
- **Daemon web:** as chaves ficam no processo do daemon (ADR-0018): só para chaves de teste.

## Onde está no código
`crates/brdu-vault`, `crates/brdu-walletd`, `apps/wallet/ui` (interface), `apps/wallet/src-tauri` (Tauri), `tools/brdu-web` (daemon), `cargo dev wallet-web`.

## Testes que cobrem
`brdu-vault/tests/vault.rs` (11: ida e volta, senha errada, adulteração de cada campo do cabeçalho e do texto cifrado, parâmetros hostis, unicode, propriedades); `brdu-walletd/tests/wallet.rs` (9, contra uma rede real de 4 validadores e o explorador: ciclo de vida, passphrase nunca guardada, importar só com endereço confirmado, exportar e trocar senha, bloqueio por inatividade e por tentativas, saldo, prévia, envio de uso único, histórico, erros claros, interface JSON sem eco de segredos); `tools/brdu-web/tests/web.rs` (4: interface com CSP estrita e todos os módulos servidos, ponte `/rpc` de ponta a ponta, **requisições hostis recusadas** — Host falso, Origin de outro site, `text/plain`, método, JSON ruim, corpo grande —, caminho fora da lista); `apps/wallet/ui/*.test.mjs` (12: teclado e formato brasileiro sem ponto flutuante, tradução dos erros, e **fluxos completos** rodando o `app.js` real: criar, enviar, importar, desbloquear, histórico, ajustes, serviço fora do ar). Verificado no navegador contra uma rede real: criar, financiar, revisar e enviar.

## Limites honestos
Chaves de teste em texto no daemon; sem WASM; sem várias contas (só a conta 0); sem QR; o app Tauri foi **compilado**, mas a janela só abre no seu computador (`cargo tauri dev`). Diagramas "a gerar".
