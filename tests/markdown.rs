//! Integration tests for the markdown lexer. Each `mod` here is a
//! file in `tests/markdown/`; collectively they exercise every non-HTML
//! CommonMark + GFM construct via the public `Lexer` / `Token` API.
//!
//! `tests/markdown.rs` is treated by Cargo as the crate root for this
//! integration-test target, so `mod foo;` would resolve relative to
//! `tests/` rather than `tests/markdown/`. We use `#[path = ...]` to keep
//! the test files grouped under `tests/markdown/`.
//!
//! Shared helper code (the `parse` helper used by nearly every file)
//! lives in `tests/markdown/common.rs`, mirroring the `tests/common/mod.rs`
//! pattern from the Rust Book.

#[path = "markdown/common.rs"]
mod common;

#[path = "markdown/autolink_extended_tests.rs"]
mod autolink_extended_tests;

#[path = "markdown/backslash_escape_tests.rs"]
mod backslash_escape_tests;

#[path = "markdown/blockquote_block_constructs_tests.rs"]
mod blockquote_block_constructs_tests;

#[path = "markdown/blockquote_inline_tests.rs"]
mod blockquote_inline_tests;

#[path = "markdown/blockquote_lazy_continuation_tests.rs"]
mod blockquote_lazy_continuation_tests;

#[path = "markdown/code_span_space_strip_tests.rs"]
mod code_span_space_strip_tests;

#[path = "markdown/collect_all_text_tests.rs"]
mod collect_all_text_tests;

#[path = "markdown/emphasis_flanking_tests.rs"]
mod emphasis_flanking_tests;

#[path = "markdown/entity_reference_tests.rs"]
mod entity_reference_tests;

#[path = "markdown/error_position_tests.rs"]
mod error_position_tests;

#[path = "markdown/fenced_code_extended_tests.rs"]
mod fenced_code_extended_tests;

#[path = "markdown/fenced_code_info_string_tests.rs"]
mod fenced_code_info_string_tests;

#[path = "markdown/gfm_trio_tests.rs"]
mod gfm_trio_tests;

#[path = "markdown/hard_break_extended_tests.rs"]
mod hard_break_extended_tests;

#[path = "markdown/html_declaration_tests.rs"]
mod html_declaration_tests;

#[path = "markdown/html_cdata_tests.rs"]
mod html_cdata_tests;

#[path = "markdown/html_processing_instruction_tests.rs"]
mod html_processing_instruction_tests;

#[path = "markdown/html_comment_block_tests.rs"]
mod html_comment_block_tests;

#[path = "markdown/html_raw_content_block_tests.rs"]
mod html_raw_content_block_tests;

#[path = "markdown/html_standalone_tag_block_tests.rs"]
mod html_standalone_tag_block_tests;

#[path = "markdown/html_block_element_tests.rs"]
mod html_block_element_tests;

#[path = "markdown/hard_line_break_tests.rs"]
mod hard_line_break_tests;

#[path = "markdown/heading_hash_in_paragraph_tests.rs"]
mod heading_hash_in_paragraph_tests;

#[path = "markdown/heading_strictness_tests.rs"]
mod heading_strictness_tests;

#[path = "markdown/image_reference_tests.rs"]
mod image_reference_tests;

#[path = "markdown/indented_code_block_tests.rs"]
mod indented_code_block_tests;

#[path = "markdown/intra_word_underscore_tests.rs"]
mod intra_word_underscore_tests;

#[path = "markdown/line_ending_normalization_tests.rs"]
mod line_ending_normalization_tests;

#[path = "markdown/link_destination_edge_tests.rs"]
mod link_destination_edge_tests;

#[path = "markdown/link_entity_decoding_tests.rs"]
mod link_entity_decoding_tests;

#[path = "markdown/link_escape_tests.rs"]
mod link_escape_tests;

#[path = "markdown/link_inline_content_tests.rs"]
mod link_inline_content_tests;

#[path = "markdown/link_title_tests.rs"]
mod link_title_tests;

#[path = "markdown/link_url_paren_and_autolink_tests.rs"]
mod link_url_paren_and_autolink_tests;

#[path = "markdown/list_lazy_continuation_tests.rs"]
mod list_lazy_continuation_tests;

#[path = "markdown/loose_tight_list_tests.rs"]
mod loose_tight_list_tests;

#[path = "markdown/multi_backtick_inline_code_tests.rs"]
mod multi_backtick_inline_code_tests;

#[path = "markdown/multi_paragraph_list_item_tests.rs"]
mod multi_paragraph_list_item_tests;

#[path = "markdown/normalize_label_tests.rs"]
mod normalize_label_tests;

#[path = "markdown/ordered_list_marker_tests.rs"]
mod ordered_list_marker_tests;

#[path = "markdown/parse_html_comment_tests.rs"]
mod parse_html_comment_tests;

#[path = "markdown/parse_image_tests.rs"]
mod parse_image_tests;

#[path = "markdown/propagate_loose_tight_tests.rs"]
mod propagate_loose_tight_tests;

#[path = "markdown/raw_inline_html_tests.rs"]
mod raw_inline_html_tests;

#[path = "markdown/reference_link_advanced_tests.rs"]
mod reference_link_advanced_tests;

#[path = "markdown/reference_link_tests.rs"]
mod reference_link_tests;

#[path = "markdown/resolve_emphasis_unit_tests.rs"]
mod resolve_emphasis_unit_tests;

#[path = "markdown/setext_and_thematic_tests.rs"]
mod setext_and_thematic_tests;

#[path = "markdown/spec_atx_heading_corners.rs"]
mod spec_atx_heading_corners;

#[path = "markdown/spec_blockquote_corners.rs"]
mod spec_blockquote_corners;

#[path = "markdown/spec_list_item_corners.rs"]
mod spec_list_item_corners;

#[path = "markdown/spec_paragraph_corners.rs"]
mod spec_paragraph_corners;

#[path = "markdown/spec_setext_corners.rs"]
mod spec_setext_corners;

#[path = "markdown/spec_tabs_corners.rs"]
mod spec_tabs_corners;

#[path = "markdown/spec_thematic_break_corners.rs"]
mod spec_thematic_break_corners;

#[path = "markdown/strikethrough_tests.rs"]
mod strikethrough_tests;

#[path = "markdown/strip_code_span_outer_space_tests.rs"]
mod strip_code_span_outer_space_tests;

#[path = "markdown/tab_expansion_tests.rs"]
mod tab_expansion_tests;

#[path = "markdown/tab_indentation_tests.rs"]
mod tab_indentation_tests;

#[path = "markdown/table_tests.rs"]
mod table_tests;

#[path = "markdown/tests.rs"]
mod tests;

#[path = "markdown/try_decode_entity_tests.rs"]
mod try_decode_entity_tests;

#[path = "markdown/try_parse_definition_tests.rs"]
mod try_parse_definition_tests;

#[path = "markdown/unmatched_emphasis_fallback_tests.rs"]
mod unmatched_emphasis_fallback_tests;
