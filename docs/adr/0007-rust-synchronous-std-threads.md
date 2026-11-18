# ADR-0007 — Rust síncrono, com threads, sem runtime assíncrono

**Status:** Aceito (escolha do plano, 2026-11-18; contestável com medição) · **Origem:** novo (padrão do Teko)

## Contexto
O objetivo do BRDU é eficiência de CPU e memória. O Teko inteiro é **síncrono** (`std::net` e threads, sem `tokio`). Um runtime assíncrono traz muitas dependências, mais código compilado (disco escasso nesta máquina) e mais memória base.

## Decisão
- Rede com `std::net` e **uma thread por sessão de par** (poucos validadores); API pública HTTP com `tiny_http`; sem `tokio`, `axum` ou `async`.
- O motor de consenso e o ledger são **funções puras, sem E/S** (testáveis e determinísticos).

## Alternativas
`tokio` + `axum`: melhor para milhares de conexões simultâneas e ecossistema maior; custo em dependências e memória. Reavaliar **se a medição** (teste de carga do M6/M10) mostrar que as threads viram o gargalo.

## Consequências
Menos dependências e binário menor. Muitas conexões de clientes simultâneas custam uma thread cada (limite por IP e total, como no Teko). O desenho não impede migrar a camada de rede depois, porque o núcleo é sem E/S.
