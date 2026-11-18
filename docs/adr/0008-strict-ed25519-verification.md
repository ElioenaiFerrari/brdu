# ADR-0008 — Verificação Ed25519 estrita, e em lote só quando segura

**Status:** Aceito (escolha do plano, 2026-11-18) · **Origem:** novo

## Contexto
O Ed25519 tem assinaturas **não canônicas** e chaves de **ordem pequena** que a verificação "frouxa" aceita. Isso permite duas assinaturas válidas para a mesma mensagem (maleabilidade) e quebra suposições de identificadores de transação. A verificação **em lote** do `ed25519-dalek` é muito mais rápida, mas **não é estrita**.

## Decisão
- Toda verificação usa `verify_strict` (recusa assinaturas não canônicas e chaves de ordem pequena).
- A verificação em lote (feature `batch`) só é usada quando o chamador diz `strict = false`. O nó, por padrão, confere estritamente cada item; o lote fica disponível para o que **não depende de unicidade da assinatura** e é medido antes de adotar.

## Consequências
A vazão por bloco é um pouco menor que a do lote puro. O ganho do lote será medido no M6 antes de qualquer mudança de padrão. Nenhuma regra de consenso depende do lote.
