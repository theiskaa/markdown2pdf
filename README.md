# markdown2pdf
markdown2pdf is a versatile command-line tool and library designed to convert Markdown content into pre-styled PDF documents. It uses a lexical analyzer to parse the Markdown and a PDF module to generate PDF documents based on the parsed tokens.

The library employs a pipeline that first tokenizes Markdown text into semantic elements (like headings, emphasis, code blocks, and lists), then processes these tokens through a styling module that applies configurable visual formatting. The styling engine supports extensive customization of fonts, colors, spacing, and other typographic properties through a TOML configuration file. For more information on how to configure the styling rules, please refer to the [Configuration](#configuration) section down below.

This project includes both a binary and a library:
- **Binary (cli)**: A command-line interface that provides an easy way to convert Markdown files, URLs, or direct string input into styled PDF documents. Supports custom styling through configuration files.
- **Library (lib)**: A robust Rust library that can be integrated into your projects for programmatic Markdown parsing and PDF generation. Offers fine-grained control over the conversion process, styling rules, and document formatting.

---

> **Note:** This project is under active development and welcomes community contributions!
> We're continuously adding new features and improvements. If you have suggestions, find bugs, or want to contribute:
> - Open an [issue](https://github.com/theiskaa/markdown2pdf/issues) for bugs or feature requests
> - Submit a [pull request](https://github.com/theiskaa/markdown2pdf/pulls) to help improve the project
> - Check our [CONTRIBUTING.md](CONTRIBUTING.md) guide for development guidelines

## Install

You can install the `markdown2pdf` binary globally using cargo by running:
```bash
cargo install markdown2pdf
```

If you want to install the latest git version:
```bash
cargo install --git https://github.com/theiskaa/markdown2pdf
```

## Install as library

Run the following Cargo command in your project directory:
```bash
cargo add markdown2pdf
```

Or add the following line to your Cargo.toml:
```toml
markdown2pdf = "0.1.4"
```

## Usage
To use the `markdown2pdf` tool, you can either specify a Markdown file path, provide Markdown content directly, or set the output PDF path.
### Options
- `-p`, `--path`: Specify the path to the Markdown file to convert.
- `-s`, `--string`: Provide Markdown content directly as a string.
- `-u`, `--url`: Specify a URL to fetch Markdown content from.
- `-o`, `--output`: Specify the output file path for the generated PDF.

### Examples
1. Convert a Markdown file to a PDF:
   ```bash
   markdown2pdf -p "docs/resume.md" -o "resume.pdf"
   ```

   Convert the 'resume.md' file in the 'docs' folder to 'resume.pdf'.

2. Convert Markdown content provided as a string:
   ```bash
   markdown2pdf -s "**bold text** *italic text*." -o "output.pdf"
   ```

   Convert the provided Markdown string to 'output.pdf'.

3. Convert Markdown from a URL:
   ```bash
   markdown2pdf -u "https://raw.githubusercontent.com/user/repo/main/README.md" -o "readme.pdf"
   ```

   Convert the Markdown content from the URL to 'readme.pdf'.

### Notes
- If multiple input options (-p, -s, -u) are provided, only one will be used in this order: path > url > string
- If no output file is specified with `-o`, the default output file will be 'output.pdf'.

## Using as Library
The library exposes two high-level functions that orchestrate the entire conversion process: `parse_into_file()` and `parse_into_bytes()`. Both functions accept raw Markdown text and handle all intermediate processing steps internally. Under the hood, they leverage the lexer to build an abstract syntax tree, apply styling rules from configuration, and render the final PDF output.

The `parse_into_file()` function is perfect for basic usage where you want to save the PDF directly to a file - simply pass your Markdown content as a string along with the desired output path. For scenarios where you need more flexibility, such as web services, API responses, or network transmission, the `parse_into_bytes()` function returns the PDF data as a byte vector that you can manipulate in memory before saving, streaming, or transmitting.

```rust
use markdown2pdf::{parse_into_file, parse_into_bytes};

// Direct file output
parse_into_file(markdown, "output.pdf", None)?;

// Get PDF as bytes for flexible handling
let pdf_bytes = parse_into_bytes(markdown, None)?;
// Use bytes as needed: save, send over network, etc.
std::fs::write("output.pdf", &pdf_bytes)?;
```

For more advanced usage, you can work directly with the lexer and PDF generation components. First, create a lexer instance to parse your Markdown content into tokens
```rust
let mut lexer = Lexer::new(markdown);
let tokens = lexer.parse().unwrap(); // handle errors
```

Next, you'll need to create a PDF renderer to transform the tokens into a formatted document. Before initializing the renderer, you'll need to define styling rules through a `StyleMatch` instance. See the [Configuration](#configuration) section below for details on customizing the styling rules.
```rust
let style = config::load_config(None);
let pdf = Pdf::new(tokens, style);
let document = pdf.render_into_document();
```

Finally, the `Document` object can be rendered to a PDF file using the `Pdf::render()` function, or converted to bytes using `Pdf::render_to_bytes()`. These functions handle the actual PDF generation, applying all the styling rules and formatting defined earlier:

## Configuration
The `markdown2pdf` tool supports customization through a TOML configuration file. You can configure various styling options for the generated PDFs by creating a `markdown2pdfrc.toml` file in your home directory, or by specifying a custom configuration file path.

Under the hood the file is translated to the `StyleMatch` instance which determines how different Markdown elements will be rendered in the final PDF. When using the library, you can load custom styling configurations using `config::load_config()` or create a custom `StyleMatch` implementation. For direct binary usage, the tool automatically looks for a configuration file in your home directory.

The configuration file supports customization of fonts, colors, spacing, and other visual properties for all Markdown elements. When using the library, you can also programmatically override these settings by modifying the `StyleMatch` instance before passing it to the PDF renderer.

### Custom Configuration Path
When using `markdown2pdf` as a library, you can specify a custom configuration file path. This is particularly useful for library usage, project-specific configurations, or when you want to maintain multiple configuration files:

Use custom config path - supports both relative and absolute paths
```rust
// For file output
markdown2pdf::parse_into_file(markdown, "output.pdf", Some("../config.toml"))?;
markdown2pdf::parse_into_file(markdown, "output.pdf", Some("/home/user/configs/style.toml"))?;

// For bytes output
let pdf_bytes = markdown2pdf::parse_into_bytes(markdown, Some("../config.toml"))?;
let pdf_bytes = markdown2pdf::parse_into_bytes(markdown, Some("/home/user/configs/style.toml"))?;
```

Use default config (~/markdown2pdfrc.toml or ./markdown2pdfrc.toml)
```rust
// For file output
markdown2pdf::parse_into_file(markdown, "output.pdf", None)?;

// For bytes output
let pdf_bytes = markdown2pdf::parse_into_bytes(markdown, None)?;
```

**Error Handling:**
If the specified configuration file cannot be found or contains invalid syntax, the library will gracefully fall back to default styling without crashing. This ensures that PDF generation always succeeds, even with configuration issues.

### Getting Started with Configuration for binary
1. Create the config file:
   ```bash
   touch ~/markdown2pdfrc.toml
   ```

2. Copy the example configuration:
   - View the example config at [markdown2pdfrc.example.toml](markdown2pdfrc.example.toml)
   - Copy the contents to your `~/markdown2pdfrc.toml` file
   - Modify the values according to your preferences

## Contributing
For information regarding contributions, please refer to [CONTRIBUTING.md](CONTRIBUTING.md) file.
