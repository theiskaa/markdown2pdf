# Changelog

All notable changes to this project will be documented in this file.
This file will include each commit message and the commit message will be grouped by
the changelog generator (git-cliff).

---

## [0.1.3] - 2025-03-31
### Features
- *(assets)* Move default implementation to [Default] trait
- *(pdf)* Implement before_spacing as breaks
- *(config)* Parse before_spacing and add in the example file
- *(styling)* Add before_spacing field to BasicTextStyle

### Bug Fixes
- *(markdown)* Enhance image parsing to handle invalid syntax gracefully
- *(lib/markdown)* Improve example doc codes
- *(markdown)* Use shorthanded struct initialization for emphasis init

### Documentation
- *(readme)* Expand library integration guide

### Testing
- *(markdown)* Add tests for standalone exclamation and image parsing
- *(lib)* Covert both success and error cases
- *(pdf)* Cover most cases except 'genpdfi' imports
- *(markdown)* Cover all possible cases of lexer
- *(styling)* Cover all cases in styling
- *(config)* Add tests for each method except reading from config file

## [0.1.2] - 2024-12-01
### Features
- *(cli)* Add URL input support for remote markdown files
- *(assets)* Embed the help text file to assets

### Bug Fixes
- *(markdown)* Remove the token printing in markdown parser

### Refactor
- *(bin)* Re-implement the structure of cli

## [0.1.1] - 2024-11-29
### Features
- *(pdf)* Implement hierarchical list rendering with proper indentation
- *(markdown)* Support mixed ordered/unordered nested lists
- *(lib)* Load embedded fonts from assets
- *(lib)* Include assets in lib
- *(lib)* Add asset embedding

### Bug Fixes
- *(markdown)* Ensure proper spacing after emphasized text
- *(pdf)* Set correct before and after settings
- *(markdown)* Handling space between tokens

### Refactor
- *(pdf)* Restructure PDF generation implementation
- *(pdf)* Improve the structure of pdf implementation

### Documentation
- *(readme)* Update the readme to have more technical info
- *(lib)* Improve code documentation

### Miscellaneous Tasks
- *(changelog)* Add "New Contributors" header to cliff
- *(cargo)* Add Cargo.lock

## New Contributors
* @orhun made their first contribution

## [0.1.0] - 2024-11-17
### Features
- *(docs)* Update readme
- *(docs)* Add contributing document
- *(base)* Use genpdfi instead of genpdf
- *(cargo)* Add version to genpdf package
- *(base)* Rename project to markdown2pdf
- *(bin)* Set lto to 'thin' and enable strip
- *(bin)* Handle the response result of parse
- *(pdf)* Improve error returning from Pdf
- *(pdf)* Handle code blocks in pdf converter
- *(markdown)* Parse multiline code blocks and code snippet language
- *(lib)* Improve documentation comments
- *(docs)* Add configuration header to readme
- *(config)* Read mdprc from the root directory
- *(lib)* Implement config parsing into library
- *(config)* Add module for parsing toml into StyleMatch
- *(config)* Add configuration toml example
- *(lib)* Add documentation comments & improve lib public methods
- *(pdf)* Call add_link for Link elements
- *(cargo)* Use fork of genpdf-rs-improved
- *(styling)* Add new roboto font & change the fonts structure
- *(styling)* Implement styling on pdf, to create pdfs based on style match
- *(styling)* Improve styling & add new paramethers and styles
- *(bin)* Add makefile for easy build
- *(styling)* Add basic styling structure
- *(bin)* Remove help.txt & add to main.rs
- *(bin)* Update both package names to mdp
- *(bin)* Update binary name to mpd
- *(bin)* Improve cli & add docummentation
- *(pdf)* Improve transforming lexer output to pdf
- *(markdown)* Make Token cloneable
- *(pdf)* Add basic logic for token to PDF element conversion
- *(pdf)* Add pdf class to convert markdown to pdf
- *(markdown)* Refactor text parsing to correctly handle special characters
- *(markdown)* Update emphasis structure to level based
- *(markdown)* Parse emphasis level correctly
- *(markdown)* Implement parsing nested tokens functionality
- *(markdown)* Bring back markdown lexer
- *(assets)* Remove test_data and move testing markdowns on local only
- *(lib)* Remove markdown lexer
- *(lexer)* Add simple lexer to parse markdown
- *(cargo)* Update the structure of cargo
- *(docs)* Add README.md
- Init cargo project

### Bug Fixes
- *(config)* Remove config path printing
- *(markdown)* Single line code block handling
- *(bin)* Update the mdp caller in main
- *(styling)* Add cross platform font path generation
- *(pdf)* Missing space after hyper links
- *(markdown)* Link item parsing

### Documentation
- *(changelog)* Add changelog generator

### Miscellaneous Tasks
- *(base)* Rename project to mdp
