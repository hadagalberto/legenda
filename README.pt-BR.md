# legenda

CLI e biblioteca Rust rápida, de binário único, para legendas de criadores.

[English](README.md)

O `legenda` lê e escreve SRT, WebVTT e ASS/SSA com tolerância e preservação
exata de bytes; aplica deslocamento, escalonamento, sincronização, reflow e
lint com timings racionais exatos; e transforma transcrições com palavras
timadas de WhisperX / whisper.cpp / OpenAI em legendas karaokê animadas em ASS
para shorts verticais, prontas para queimar com ffmpeg.

- **Detecção de codificação interna**: UTF-8 (com BOM), UTF-16 LE/BE (com BOM) e
  fallback windows-1252 para legendas pt-BR legadas — sem crates externas.
- **Tempo sem ponto flutuante**: cada instante é um racional exato, então
  conversões 23,976 ↔ 25 fps e ida-e-volta de shift não perdem informação.
- **Regras publicadas**: o preset `netflix-ptbr` usa os valores oficiais da
  Netflix (42 caracteres por linha, 2 linhas, 17 CPS adulto / 13 CPS infantil,
  5/6 s a 7 s). As fontes estão em
  [docs/netflix-ptbr-style-guide.md](docs/netflix-ptbr-style-guide.md).

Licença MIT. Autor: Hadagalberto Junior.

## Instalação

Requer Rust estável (MSRV 1.85). Nenhuma biblioteca do sistema é necessária
para compilar a CLI; o ffmpeg só é usado no comando `burn` e no teste
end-to-end.

```sh
git clone https://github.com/HadagalbertoJunior/legenda.git
cd legenda
cargo build --release
# binário: target/release/legenda  (target\release\legenda.exe no Windows)
```

Para instalar no diretório de binários do cargo:

```sh
cargo install --path legenda
legenda --help
```

Para rodar a suite completa de qualidade a partir do checkout:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --release
```

## Visão dos comandos

| Comando | Finalidade |
| --- | --- |
| `convert ENTRADA SAÍDA` | Converte entre SRT, VTT e ASS (formato pela extensão; `-f`/`-t` sobrescrevem). |
| `info ENTRADA` | Mostra formato, codificação detectada, nº de falas e intervalo. |
| `shift ENTRADA SAÍDA --by 1.5s` | Desloca todas as falas; segundos/ms com sinal ou timestamp. |
| `scale ENTRADA SAÍDA --from 23.976 --to 25` | Conversão exata de framerate. |
| `sync ENTRADA SAÍDA --from-a … --to-a … --from-b … --to-b …` | Sincronização afim por dois pontos. |
| `clamp ENTRADA SAÍDA --start … --end …` | Mantém só as falas que intersectam o intervalo. |
| `merge A B -o SAÍDA` / `split ENTRADA A B --at …` | Junta em ordem cronológica / divide. |
| `strip-hi`, `strip-speakers`, `clean` | Remove `[música]`, `(risos)`, `♪…♪` e/ou rótulos `JOÃO:`. |
| `lint ENTRADA --json [--preset …] [--fps …]` | Relata violações; saída 1 se sujo, 2 se erro. |
| `reflow ENTRADA SAÍDA --preset netflix-ptbr\|shorts` | Quebras linguísticas, CPS e duração. |
| `karaoke TRANSCRIÇÃO.json -o SAÍDA.ass --preset …` | Legendas ASS animadas por palavra. |
| `burn VÍDEO LEGENDA.ass -o SAÍDA.mp4` | Queima o ASS no vídeo com ffmpeg. |

Use `legenda <comando> --help` para ver todas as opções.

## Receita: WhisperX → legendas de shorts → burn

Suponha `video.mp4` (vertical, 1080×1920) e um JSON do WhisperX com
`segments[].words[]` contendo `word`, `start`, `end` e `score`.

```sh
# 1. Gera páginas ASS animadas (uma página por ~8 palavras, pontuação ou pausa).
legenda karaoke whisperx.json -o shorts.ass --preset hormozi

# 2. Opcional: ajusta tipografia e área segura das plataformas.
legenda karaoke whisperx.json -o shorts.ass --preset bounce \
  --font Anton --font-size 96 --outline 6 \
  --safe-top 180 --safe-bottom 420 \
  --emphasis nunca --emphasis jamais

# 3. Queima com ffmpeg (codificador de software por padrão).
legenda burn video.mp4 shorts.ass -o reel.mp4

# Codificador de hardware (opções repassadas literalmente):
legenda burn video.mp4 shorts.ass -o reel.mp4 \
  --encoder h264_nvenc --encoder-option preset=p4 --encoder-option crf=28
```

Outros formatos de transcrição funcionam com o mesmo comando: JSON completo do
whisper.cpp `-ojf` (`transcription[].tokens[].offsets`, em milissegundos) e
`verbose_json` da OpenAI (`words[]`, em segundos). Tempos de palavras ausentes
são interpolados dentro do segmento. Presets: `hormozi` (maiúsculas em negrito
com destaque colorido), `clean` (revelação suave sobre caixa sutil), `karaoke`
(preenchimento `\k`) e `bounce` (salto com amortecimento). Emojis passam
intactos; chaves são escapadas.

O arquivo de legendas é copiado para um nome temporário ASCII antes da queima,
então metacaracteres de filtro (`, ; [ ] '`), espaços e acentos nos caminhos
nunca chegam ao escape de dois níveis do ffmpeg. Os caminhos de vídeo e saída
são passados literalmente. Funciona em Linux, macOS e Windows.

## Receita: reflow no estilo Netflix

```sh
# Aplica o guia pt-BR publicado a 25 fps.
legenda reflow legendas.srt legendas-reflow.srt \
  --preset netflix-ptbr --fps 25

# Velocidade de leitura infantil: 13 CPS em vez de 17.
legenda reflow legendas.srt legendas-criancas.srt \
  --preset netflix-ptbr-children --fps 25

# Verifica o resultado: 0 = limpo, 1 = violações, 2 = erro.
legenda lint legendas-reflow.srt --preset netflix-ptbr --fps 25 --json
```

O reflow quebra depois de pontuação e antes de conjunções/preposições, nunca
separa artigo de substantivo, equilibra as linhas com base inferior, limita
cada fala a 5/6 s–7 s, aplica 17/13 CPS estendendo falas e deslocando as
seguintes, e mantém intervalo mínimo de dois quadros. Falas longas demais são
divididas em fronteiras de palavra; nenhuma palavra é perdida. O preset
`shorts` usa uma linha por página de cerca de quatro palavras.

## Receita: lint no CI

O `lint` gera saída legível por máquina e usa o código de saída para falhar o
build:

```yaml
- name: Verifica timing das legendas
  shell: bash
  run: |
    legenda lint captions/pt-BR.srt --json > lint-report.json || code=$?
    cat lint-report.json
    # 0 = limpo, 1 = violações (falha o job), 2 = erro de uso/IO
    test "${code:-0}" -le 1
    test "${code:-0}" -ne 1
```

Passo mais simples para um único arquivo:

```yaml
- name: Lint das legendas
  run: cargo run --quiet --bin legenda -- lint captions/pt-BR.srt --fps 25
```

O JSON contém `cues`, `violations`, `by_rule` e `issues`. Regras:
`invalid_timing`, `cps`, `overlap`, `gap`, `too_short`, `too_long`,
`line_length`, `too_many_lines`.

## Biblioteca

```rust
use legenda_core::{Format, Subtitle};

let bytes = std::fs::read("captions.srt")?;
let subtitles = Subtitle::decode(&bytes, Format::Srt)?; // codificação detectada
let cues = subtitles.plain_cues()?;
let vtt = legenda_core::convert::convert(&subtitles, Format::Vtt)?;
std::fs::write("captions.vtt", vtt.encode()?)?;
```

## Estrutura do projeto

- `legenda-core/` — biblioteca: parsers, writers, conversão, tempo, codificação,
  operações, reflow, lint, leitura de transcrições, karaokê e argumentos do ffmpeg.
- `legenda/` — CLI derivada de clap.
- `legenda-core/tests/fixtures/` — corpus com 57 arquivos byte-exatos de
  SRT/VTT/ASS. Os snapshots insta do modelo parseado e da saída reescrita ficam
  em `legenda-core/tests/snapshots/`.

Veja [CONTRIBUTING.md](CONTRIBUTING.md) para desenvolvimento e testes, e
[CHANGELOG.md](CHANGELOG.md) para o histórico de versões.

## Licença

MIT — veja [LICENSE](LICENSE).
