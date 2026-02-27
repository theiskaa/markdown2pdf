## [0.2.2] - 2026-02-27

### Features

- *(fonts)* Embed minimal TrueType font for built-in metrics fallback
- *(fonts)* Add FontSource::bytes() constructor and document priority behavior

### Bug Fixes

- *(lib)* Propagate Pdf::new errors in parse_into_file and parse_into_bytes
- *(pdf)* Replace .expect() with graceful fallback on font source loading

### Other

- Don't check builtinfirst if providing font source
- Allow loading a font as bytes

### Refactor

- *(pdf)* Replace .expect() panics with Result propagation in Pdf::new
- *(fonts)* Remove section separator comments


### Documentation

- *(readme)* Note that built-in fonts work without system fonts installed
- *(readme)* Add embedded bytes font loading mode and usage example

## [0.2.1] - 2026-01-27

### Features

- *(pdf)* Apply bold/italic/underline/strikethrough styles to links

### Testing

- *(lib)* Add link styling tests for underline and strikethrough

### Miscellaneous Tasks

- *(cargo)* Update version to v0.2.1 and use genpdfi v0.2.7
- *(docs)* Remove donation information and related funding references

## [0.2.0] - 2026-01-27

### Features

- *(dependencies)* Use new genpdfi version 0.2.6
- *(styling,config)* Add basic styling config to table header rows and cells
- *(pdf)* Implement table rendering
- *(debug)* Add table debug
- *(markdown)* Support tables

### Bug Fixes

- *(ci)* Allow dirty workflow for manual trigger support
- *(ci)* Workflow_dispatch to run in plan mode
- *(markdown)* Only use text tokens in table cells
- *(markdown)* Ignore `>` again in parse_text

### Refactor

- *(fonts)* Simplify font system and remove fontdb dependency
- *(bin)* Update argument conflict handling for markdown options
- *(fonts)* Implement global cached font database
- *(logging)* Replace eprintln! with log macros for improved logging
- *(markdown)* Intruduce ParseContext for context-aware parsing

### Documentation

- *(README)* Update Markdown features and font handling details
- *(readme)* Add logging section to documentation for log crate integration

### Performance

- *(pdf)* Reduce overhead in rendering
- *(fonts)* Speed up font loading

### Testing

- *(markdown)* Add test for table parsing

### Miscellaneous Tasks

- *(ci)* Add manual workflow trigger
- *(ci)* Update macos runner version to 14
- *(release)* Update cargo-dist version to 0.30.3 in configuration and CI workflow
- *(dependendies)* Update cargo lock
- *(release)* Update version to v0.2.0
- *(dependencies)* Add log:0.4 dependency to cargo

## [0.1.9] - 2025-11-14

### Features

- _(fonts)_ Improve font loading by adding validation for font data and handling of .ttc files
- _(pdf)_ Improve font loading by applying subsetting to fallback chains based on character usage
- _(fonts)_ Add font subsetting functionality to reduce PDF file size by analyzing character usage in fallback chains
- _(pdf)_ Improve font loading with automatic fallback chains and improved handling for missing fonts
- _(fonts)_ Improve font loading with fallback chains and improved error reporting for missing fonts
- _(cli)_ Add verbosity levels and dry-run option for enhanced user feedback during PDF generation
- _(errors)_ Improve MdpError handling with detailed error structures and suggestions for better debugging
- _(fonts)_ Improve font loading with aliases and variant support for better fallback handling
- _(validation)_ Implement a validation system for markdown conversion with warnings for missing fonts, images, and syntax issues
- _(pdf)_ Enhance font loading with optional text extraction for improved fallback handling
- _(markdown)_ Add recursive text extraction method for tokens
- _(fonts)_ Add font subsetting support
- _(lib)_ Update parse_into_file and parse_into_bytes to support optional font configuration
- _(pdf)_ Improve pdf creation with optional font configuration for custom and default fonts
- _(fonts)_ Implement custom font loading with configuration options
- _(bin)_ Add font configuration options for custom and default fonts

### Bug Fixes

- _(pdf)_ Change default behavior of font subsetting to true for improved text extraction

### Documentation

- _(readme)_ Update feature flag section
- _(readme)_ Simplfiy readme to make it more pleasant to read

### Miscellaneous Tasks

- _(cargo)_ Add feature flags to control the lib by preference
- _(.gitignore)_ Add pattern to ignore test markdown files
- _(dependencies)_ Update dependencies in Cargo.lock and Cargo.toml, including version bump for markdown2pdf to 0.1.9 and switching genpdfi to a git source for font subsetting feature
- _(docs)_ Update README to improve docs on cli options, font handling, and Unicode support

## [0.1.8] - 2025-10-07

### Features

- _(markdown)_ Improve list parsing and add is_list_marker method for better list item handling
- _(debug)_ Add functionality to save tokens as JSON for visualization

### Bug Fixes

- _(markdown)_ Remove the token visualization json generator call

### Other

- Version v0.1.8

### Documentation

- _(changelog)_ Update changelog with 0.1.7 commits
- _(readme)_ Update readme to include homebrew and prebuilt install variants

### Miscellaneous Tasks

- Release 0.1.8 version

## [0.1.7] - 2025-09-21

### Other

- Version 0.1.7

### Documentation

- _(funding)_ Add github funding file
- _(donate)_ Add donation information and link to DONATE.md in README

### Miscellaneous Tasks

- _(ci)_ Add GitHub Actions workflow for release automation and configure dist settings in Cargo.toml and new dist-workspace.toml

## [0.1.6] - 2025-07-21

### Features

- _(config)_ Add ConfigSource enum and refactor configuration loading functions to support default, file, and embedded sources

### Bug Fixes

- _(main)_ Update parse_into_file call to use ConfigSource::Default for improved configuration handling

### Refactor

- _(lib)_ Update parse_into_file and parse_into_bytes functions to use ConfigSource for configuration handling

### Documentation

- _(readme)_ Add new logo and some cool badges
- _(readme)_ Revise README for clarity and update configuration handling details, including embedded support and usage examples

### Miscellaneous Tasks

- _(cargo)_ Update version to 0.1.6 and reflect changes in Cargo.toml, Cargo.lock, CHANGELOG.md, and README.md

## [0.1.5] - 2025-07-16

### Features

- _(lib)_ Rename parse function to parse_into_file and add parse_into_bytes for in-memory PDF generation
- _(pdf)_ Add render_to_bytes method for in-memory PDF generation and corresponding tests

### Documentation

- _(readme)_ Update documentation to reflect new parse_into_file and parse_into_bytes functions for PDF generation
- _(readme)_ Update version from 0.1.3 to 0.1.4 in readme

### Miscellaneous Tasks

- _(cargo)_ Update version to 0.1.5 and reflect version changes in other files
- _(app)_ Remove Makefile at all

## [0.1.4] - 2025-07-09

### Features

- _(fonts)_ Add comprehensive font loading functionality with built-in and system font support
- _(fonts)_ Implement ultra-minimal font loading with caching and variant analysis

### Bug Fixes

- _(main)_ Update error handling to provide clearer user guidance for markdown input requirements

### Other

- _(cargo)_ Remove thiserror module

### Refactor

- _(lib)_ Update parse function to accept an optional configuration path for improved flexibility
- _(main)_ Improve command handling and error messaging for markdown input
- _(lib)_ Remove commented sections and unused font loading logic
- _(styling)_ Remove unused font references and optimize font loading logic for efficiency
- _(pdf)_ Enhance font loading logic to support system fonts and fallback options
- _(pdf)_ Replace font loading methods with minimal variants for improved efficiency

### Documentation

- _(readme)_ Enhance configuration section to include custom config file path and error handling details

### Miscellaneous Tasks

- _(dependencies)_ Update Cargo.lock
- _(cargo)_ Update version from v0.1.3 to v0.1.4
- _(dependencies)_ Update genpdfi source from github to registry (version 0.2.3 )
- _(dependencies)_ Update package versions and add new dependencies for font handling and PDF generation
- _(lib)_ Remove embedded assets including help text and Roboto font files to streamline the project and reduce binary size.
- Fix some advisory issues
- Make it possible to compile without openssl
- _(dependencies)_ Bump genpdfi version to 0.2.2 in Cargo.toml and Cargo.lock

## [0.1.3] - 2025-03-31

### Features

- _(assets)_ Move default implementation to [Default] trait
- _(pdf)_ Implement before_spacing as breaks
- _(config)_ Parse before_spacing and add in the example file
- _(styling)_ Add before_spacing field to BasicTextStyle

### Bug Fixes

- _(markdown)_ Enhance image parsing to handle invalid syntax gracefully
- _(lib/markdown)_ Improve example doc codes
- _(markdown)_ Use shorthanded struct initialization for emphasis init

### Documentation

- _(changelog)_ Update CHANGELOG.md for version 0.1.3 with new features, bug fixes, documentation updates, and tests
- _(readme)_ Expand library integration guide

### Testing

- _(markdown)_ Add tests for standalone exclamation and image parsing
- _(lib)_ Covert both success and error cases
- _(pdf)_ Cover most cases except 'genpdfi' imports
- _(markdown)_ Cover all possible cases of lexer
- _(styling)_ Cover all cases in styling
- _(config)_ Add tests for each method except reading from config file

### Miscellaneous Tasks

- _(lib)_ Update markdown2pdf version to 0.1.3 in Cargo.toml and Cargo.lock

## [0.1.2] - 2024-12-01

### Features

- _(cli)_ Add URL input support for remote markdown files
- _(assets)_ Embed the help text file to assets

### Bug Fixes

- _(markdown)_ Remove the token printing in markdown parser

### Refactor

- _(bin)_ Re-implement the structure of cli

### Miscellaneous Tasks

- _(release)_ Bump version to 0.1.2

## [0.1.1] - 2024-11-29

### Features

- _(lib)_ Bump new version v0.1.1
- _(pdf)_ Implement hierarchical list rendering with proper indentation
- _(markdown)_ Support mixed ordered/unordered nested lists
- _(lib)_ Load embedded fonts from assets
- _(lib)_ Include assets in lib
- _(lib)_ Add asset embedding

### Bug Fixes

- _(markdown)_ Ensure proper spacing after emphasized text
- _(pdf)_ Set correct before and after settings
- _(markdown)_ Handling space between tokens

### Refactor

- _(pdf)_ Restructure PDF generation implementation
- _(pdf)_ Improve the structure of pdf implementation

### Documentation

- _(readme)_ Update the readme to have more technical info
- _(lib)_ Improve code documentation

### Miscellaneous Tasks

- _(changelog)_ Add "New Contributors" header to cliff
- _(cargo)_ Add Cargo.lock

## [0.1.0] - 2024-11-17

### Features

- _(docs)_ Update readme
- _(docs)_ Add contributing document
- _(base)_ Use genpdfi instead of genpdf
- _(cargo)_ Add version to genpdf package
- _(base)_ Rename project to markdown2pdf
- _(bin)_ Set lto to 'thin' and enable strip
- _(bin)_ Handle the response result of parse
- _(pdf)_ Improve error returning from Pdf
- _(pdf)_ Handle code blocks in pdf converter
- _(markdown)_ Parse multiline code blocks and code snippet language
- _(lib)_ Improve documentation comments
- _(docs)_ Add configuration header to readme
- _(config)_ Read mdprc from the root directory
- _(lib)_ Implement config parsing into library
- _(config)_ Add module for parsing toml into StyleMatch
- _(config)_ Add configuration toml example
- _(lib)_ Add documentation comments & improve lib public methods
- _(pdf)_ Call add_link for Link elements
- _(cargo)_ Use fork of genpdf-rs-improved
- _(styling)_ Add new roboto font & change the fonts structure
- _(styling)_ Implement styling on pdf, to create pdfs based on style match
- _(styling)_ Improve styling & add new paramethers and styles
- _(bin)_ Add makefile for easy build
- _(styling)_ Add basic styling structure
- _(bin)_ Remove help.txt & add to main.rs
- _(bin)_ Update both package names to mdp
- _(bin)_ Update binary name to mpd
- _(bin)_ Improve cli & add docummentation
- _(pdf)_ Improve transforming lexer output to pdf
- _(markdown)_ Make Token cloneable
- _(pdf)_ Add basic logic for token to PDF element conversion
- _(pdf)_ Add pdf class to convert markdown to pdf
- _(markdown)_ Refactor text parsing to correctly handle special characters
- _(markdown)_ Update emphasis structure to level based
- _(markdown)_ Parse emphasis level correctly
- _(markdown)_ Implement parsing nested tokens functionality
- _(markdown)_ Bring back markdown lexer
- _(assets)_ Remove test_data and move testing markdowns on local only
- _(lib)_ Remove markdown lexer
- _(lexer)_ Add simple lexer to parse markdown
- _(cargo)_ Update the structure of cargo
- _(docs)_ Add README.md
- Init cargo project

### Bug Fixes

- _(config)_ Remove config path printing
- _(markdown)_ Single line code block handling
- _(bin)_ Update the mdp caller in main
- _(styling)_ Add cross platform font path generation
- _(pdf)_ Missing space after hyper links
- _(markdown)_ Link item parsing

### Documentation

- _(changelog)_ Add changelog generator

### Miscellaneous Tasks

- _(base)_ Rename project to mdp
