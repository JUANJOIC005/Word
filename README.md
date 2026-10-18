<p align="center">
  <img alt="Word" src="assets/app-icon/hicolor/256x256/apps/io.github.juanjoic005.word.png" width="128">
</p>

<h1 align="center">Word</h1>

<p align="center">
  Procesador de textos nativo y de código abierto, escrito en Rust, con el flujo de trabajo que ya conoces:
  cinta de opciones, estilos, tablas, control de cambios, referencias y combinación de correspondencia.
</p>

> **Fork personal.** Este repositorio es una copia independiente de
> [storytold/wordcraft](https://github.com/storytold/wordcraft) (versión 0.8.0 como base) con
> la identidad visible cambiada a **Word**, icono propio y compilaciones automáticas en GitHub Actions.
> No está afiliado a Microsoft ni al equipo original. «Word» aquí es solo el nombre de este fork; el icono
> es original y no usa la marca de Microsoft Word.

## Qué es

- Lee y escribe `.docx`, abre `.odt`, `.rtf`, `.html`, `.md` y `.txt`, y exporta a PDF.
- Aplicación de escritorio (Linux, Windows) y herramienta de línea de comandos `word-cli`.
- Interfaz y motor 100 % Rust (egui/wgpu); sin servicios en la nube.

## Asistente IA (propio de este fork)

**Ayuda › Asistente IA** abre un panel lateral para que **Claude Code** o **Codex** escriban y editen
en directo el documento abierto:

- Crear un documento desde cero (en blanco, carta, currículum o informe) a partir de una descripción.
- Adjuntar archivos (texto, PDF, imágenes…) que el agente lee; las imágenes se pueden insertar.
- Acciones rápidas: resumir, corregir, mejorar formato, tabla de contenido, seguir escribiendo, traducir.
- Opciones: modelo de Claude, trabajar solo sobre la selección y marcar sus cambios con control de cambios.

Usa tu propia instalación de `claude` o `codex` (tu cuenta y tu plan). La app arranca su canal de
control local y conecta el agente con `word-cli mcp --connect`; el código está en
`crates/ui-egui/src/ai_pane.rs`.

## Descargar e instalar

Las compilaciones se publican en [Releases](https://github.com/JUANJOIC005/Word/releases).

**Linux (Debian/Ubuntu y derivadas, x86_64):**

```sh
sudo apt install ./Word-<versión>-linux-x86_64.deb
word            # abre la aplicación
word-cli --help # herramienta de línea de comandos
```

Windows se compila con los workflows de `.github/workflows` (artefactos de cada ejecución).

## Compilar

Todo se compila en GitHub Actions, no hace falta toolchain local. El workflow
[`release.yml`](.github/workflows/release.yml) (manual con *Run workflow*, o al subir una etiqueta `v*`)
usa Rust 1.95 (edición 2024) con caché de cargo, genera el `.deb` con `packaging/linux/package.sh`
y lo adjunta como artefacto y como asset de la Release.

Para compilar a mano: `cargo build --release -p wordcraft -p wordcraft-cli` (los binarios compilados se
llaman `wordcraft`/`wordcraft-cli`; el empaquetado los instala como `word`/`word-cli`).

## Relación con el proyecto original

- Los crates internos conservan el prefijo `wordcraft-*` a propósito: así las mejoras del proyecto
  original se incorporan con pocos conflictos.
- Lo que cambia en este fork: nombre visible, icono, identificador de escritorio
  (`io.github.juanjoic005.word`), nombres de paquete y ejecutables, enlaces y workflows de release.

Para traer novedades del original:

```sh
git remote add upstream https://github.com/storytold/wordcraft.git
git fetch upstream
git merge upstream/main   # resolver conflictos de nombre/identidad si los hay
```

## Licencia y atribución

MIT OR Apache-2.0 (ver [`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE) y
[`NOTICE`](NOTICE)). Software original © sus colaboradores; terceros y fuentes en
[`ATTRIBUTION.md`](ATTRIBUTION.md). Los logotipos de ArtCraft se han eliminado de este fork.
