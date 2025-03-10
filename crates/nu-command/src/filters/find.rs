use crate::help::highlight_search_string;
use itertools::Itertools;

use fancy_regex::Regex;
use nu_ansi_term::Style;
use nu_color_config::StyleComputer;
use nu_engine::CallExt;
use nu_protocol::{
    ast::Call,
    engine::{Command, EngineState, Stack},
    record, Category, Config, Example, IntoInterruptiblePipelineData, IntoPipelineData, ListStream,
    PipelineData, Record, ShellError, Signature, Span, SyntaxShape, Type, Value,
};

#[derive(Clone)]
pub struct Find;

impl Command for Find {
    fn name(&self) -> &str {
        "find"
    }

    fn signature(&self) -> Signature {
        Signature::build(self.name())
            .input_output_types(vec![
                (
                    // TODO: This is too permissive; if we could express this
                    // using a type parameter it would be List<T> -> List<T>.
                    Type::List(Box::new(Type::Any)),
                    Type::List(Box::new(Type::Any)),
                ),
                (Type::String, Type::Any),
                (
                    // For find -p
                    Type::Table(vec![]),
                    Type::Table(vec![]),
                ),
            ])
            .named(
                "regex",
                SyntaxShape::String,
                "regex to match with",
                Some('r'),
            )
            .switch(
                "ignore-case",
                "case-insensitive regex mode; equivalent to (?i)",
                Some('i'),
            )
            .switch(
                "multiline",
                "multi-line regex mode: ^ and $ match begin/end of line; equivalent to (?m)",
                Some('m'),
            )
            .switch(
                "dotall",
                "dotall regex mode: allow a dot . to match newlines \\n; equivalent to (?s)",
                Some('s'),
            )
            .named(
                "columns",
                SyntaxShape::List(Box::new(SyntaxShape::String)),
                "column names to be searched (with rest parameter, not regex yet)",
                Some('c'),
            )
            .switch("invert", "invert the match", Some('v'))
            .rest("rest", SyntaxShape::Any, "terms to search")
            .category(Category::Filters)
    }

    fn usage(&self) -> &str {
        "Searches terms in the input."
    }

    fn examples(&self) -> Vec<Example> {
        vec![
            Example {
                description: "Search for multiple terms in a command output",
                example: r#"ls | find toml md sh"#,
                result: None,
            },
            Example {
                description: "Search for a term in a string",
                example: r#"'Cargo.toml' | find toml"#,
                result: Some(Value::test_string("Cargo.toml".to_owned())),
            },
            Example {
                description: "Search a number or a file size in a list of numbers",
                example: r#"[1 5 3kb 4 3Mb] | find 5 3kb"#,
                result: Some(Value::list(
                    vec![Value::test_int(5), Value::test_filesize(3000)],
                    Span::test_data(),
                )),
            },
            Example {
                description: "Search a char in a list of string",
                example: r#"[moe larry curly] | find l"#,
                result: Some(Value::list(
                    vec![Value::test_string("larry"), Value::test_string("curly")],
                    Span::test_data(),
                )),
            },
            Example {
                description: "Find using regex",
                example: r#"[abc bde arc abf] | find --regex "ab""#,
                result: Some(Value::list(
                    vec![
                        Value::test_string("abc".to_string()),
                        Value::test_string("abf".to_string()),
                    ],
                    Span::test_data(),
                )),
            },
            Example {
                description: "Find using regex case insensitive",
                example: r#"[aBc bde Arc abf] | find --regex "ab" -i"#,
                result: Some(Value::list(
                    vec![
                        Value::test_string("aBc".to_string()),
                        Value::test_string("abf".to_string()),
                    ],
                    Span::test_data(),
                )),
            },
            Example {
                description: "Find value in records using regex",
                example: r#"[[version name]; ['0.1.0' nushell] ['0.1.1' fish] ['0.2.0' zsh]] | find --regex "nu""#,
                result: Some(Value::test_list(
                    vec![Value::test_record(record! {
                            "version" => Value::test_string("0.1.0"),
                            "name" => Value::test_string("nushell".to_string()),
                    })],
                )),
            },
            Example {
                description: "Find inverted values in records using regex",
                example: r#"[[version name]; ['0.1.0' nushell] ['0.1.1' fish] ['0.2.0' zsh]] | find --regex "nu" --invert"#,
                result: Some(Value::test_list(
                    vec![
                        Value::test_record(record!{
                                "version" => Value::test_string("0.1.1"),
                                "name" => Value::test_string("fish".to_string()),
                        }),
                        Value::test_record(record! {
                                "version" => Value::test_string("0.2.0"),
                                "name" =>Value::test_string("zsh".to_string()),
                        }),
                    ],
                )),
            },
            Example {
                description: "Find value in list using regex",
                example: r#"[["Larry", "Moe"], ["Victor", "Marina"]] | find --regex "rr""#,
                result: Some(Value::list(
                    vec![Value::list(
                        vec![Value::test_string("Larry"), Value::test_string("Moe")],
                        Span::test_data(),
                    )],
                    Span::test_data(),
                )),
            },
            Example {
                description: "Find inverted values in records using regex",
                example: r#"[["Larry", "Moe"], ["Victor", "Marina"]] | find --regex "rr" --invert"#,
                result: Some(Value::list(
                    vec![Value::list(
                        vec![Value::test_string("Victor"), Value::test_string("Marina")],
                        Span::test_data(),
                    )],
                    Span::test_data(),
                )),
            },
            Example {
                description: "Remove ANSI sequences from result",
                example: "[[foo bar]; [abc 123] [def 456]] | find 123 | get bar | ansi strip",
                result: None, // This is None because ansi strip is not available in tests
            },
            Example {
                description: "Find and highlight text in specific columns",
                example:
                    "[[col1 col2 col3]; [moe larry curly] [larry curly moe]] | find moe --columns [col1]",
                result: Some(Value::list(
                    vec![Value::test_record(record! {
                            "col1" => Value::test_string(
                                "\u{1b}[37m\u{1b}[0m\u{1b}[41;37mmoe\u{1b}[0m\u{1b}[37m\u{1b}[0m"
                                    .to_string(),
                            ),
                            "col2" => Value::test_string("larry".to_string()),
                            "col3" => Value::test_string("curly".to_string()),
                    })],
                    Span::test_data(),
                )),
            },
        ]
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["filter", "regex", "search", "condition"]
    }

    fn run(
        &self,
        engine_state: &EngineState,
        stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        let regex = call.get_flag::<String>(engine_state, stack, "regex")?;

        if let Some(regex) = regex {
            find_with_regex(regex, engine_state, stack, call, input)
        } else {
            let input = split_string_if_multiline(input, call.head);
            find_with_rest_and_highlight(engine_state, stack, call, input)
        }
    }
}

fn find_with_regex(
    regex: String,
    engine_state: &EngineState,
    _stack: &mut Stack,
    call: &Call,
    input: PipelineData,
) -> Result<PipelineData, ShellError> {
    let span = call.head;
    let ctrlc = engine_state.ctrlc.clone();
    let config = engine_state.get_config().clone();

    let insensitive = call.has_flag("ignore-case");
    let multiline = call.has_flag("multiline");
    let dotall = call.has_flag("dotall");
    let invert = call.has_flag("invert");

    let flags = match (insensitive, multiline, dotall) {
        (false, false, false) => "",
        (true, false, false) => "(?i)", // case insensitive
        (false, true, false) => "(?m)", // multi-line mode
        (false, false, true) => "(?s)", // allow . to match \n
        (true, true, false) => "(?im)", // case insensitive and multi-line mode
        (true, false, true) => "(?is)", // case insensitive and allow . to match \n
        (false, true, true) => "(?ms)", // multi-line mode and allow . to match \n
        (true, true, true) => "(?ims)", // case insensitive, multi-line mode and allow . to match \n
    };

    let regex = flags.to_string() + regex.as_str();

    let re = Regex::new(regex.as_str()).map_err(|e| ShellError::TypeMismatch {
        err_message: format!("invalid regex: {e}"),
        span,
    })?;

    input.filter(
        move |value| match value {
            Value::String { val, .. } => re.is_match(val.as_str()).unwrap_or(false) != invert,
            Value::Record {
                val: Record { vals, .. },
                ..
            }
            | Value::List { vals, .. } => values_match_find(vals, &re, &config, invert),
            _ => false,
        },
        ctrlc,
    )
}

fn values_match_find(values: &[Value], re: &Regex, config: &Config, invert: bool) -> bool {
    match invert {
        true => !record_matches_regex(values, re, config),
        false => record_matches_regex(values, re, config),
    }
}

fn record_matches_regex(values: &[Value], re: &Regex, config: &Config) -> bool {
    values.iter().any(|v| {
        re.is_match(v.into_string(" ", config).as_str())
            .unwrap_or(false)
    })
}

#[allow(clippy::too_many_arguments)]
fn highlight_terms_in_record_with_search_columns(
    search_cols: &[String],
    record: &Record,
    span: Span,
    config: &Config,
    terms: &[Value],
    string_style: Style,
    highlight_style: Style,
) -> Value {
    let cols_to_search = if search_cols.is_empty() {
        &record.cols
    } else {
        search_cols
    };
    let term_strs: Vec<_> = terms.iter().map(|v| v.into_string("", config)).collect();

    // iterator of Ok((val_str, term_str)) pairs if the value should be highlighted, otherwise Err(val)
    let try_val_highlight = record.iter().map(|(col, val)| {
        let val_str = val.into_string("", config);
        let predicate = cols_to_search.contains(col);
        predicate
            .then_some(val_str)
            .and_then(|val_str| {
                term_strs
                    .iter()
                    .find(|term_str| contains_ignore_case(&val_str, term_str))
                    .map(|term_str| (val_str, term_str))
            })
            .ok_or_else(|| val.clone())
    });

    // turn Ok pairs into vals of highlighted strings, Err vals is original vals
    let new_vals = try_val_highlight
        .map_ok(|(val_str, term_str)| {
            let highlighted_str =
                highlight_search_string(&val_str, term_str, &string_style, &highlight_style)
                    .unwrap_or_else(|_| string_style.paint(term_str).to_string());

            Value::string(highlighted_str, span)
        })
        .map(|v| v.unwrap_or_else(|v| v));

    Value::record(
        Record {
            cols: record.cols.clone(),
            vals: new_vals.collect(),
        },
        span,
    )
}

fn contains_ignore_case(string: &str, substring: &str) -> bool {
    string.to_lowercase().contains(&substring.to_lowercase())
}

fn find_with_rest_and_highlight(
    engine_state: &EngineState,
    stack: &mut Stack,
    call: &Call,
    input: PipelineData,
) -> Result<PipelineData, ShellError> {
    let span = call.head;
    let ctrlc = engine_state.ctrlc.clone();
    let engine_state = engine_state.clone();
    let config = engine_state.get_config().clone();
    let filter_config = engine_state.get_config().clone();
    let invert = call.has_flag("invert");
    let terms = call.rest::<Value>(&engine_state, stack, 0)?;
    let lower_terms = terms
        .iter()
        .map(|v| Value::string(v.into_string("", &config).to_lowercase(), span))
        .collect::<Vec<Value>>();

    let style_computer = StyleComputer::from_config(&engine_state, stack);
    // Currently, search results all use the same style.
    // Also note that this sample string is passed into user-written code (the closure that may or may not be
    // defined for "string").
    let string_style = style_computer.compute("string", &Value::string("search result", span));
    let highlight_style =
        style_computer.compute("search_result", &Value::string("search result", span));

    let cols_to_search_in_map = match call.get_flag(&engine_state, stack, "columns")? {
        Some(cols) => cols,
        None => vec![],
    };

    let cols_to_search_in_filter = cols_to_search_in_map.clone();

    match input {
        PipelineData::Empty => Ok(PipelineData::Empty),
        PipelineData::Value(_, _) => input
            .map(
                move |mut x| {
                    let span = x.span();
                    match &mut x {
                        Value::Record { val, .. } => highlight_terms_in_record_with_search_columns(
                            &cols_to_search_in_map,
                            val,
                            span,
                            &config,
                            &terms,
                            string_style,
                            highlight_style,
                        ),
                        _ => x,
                    }
                },
                ctrlc.clone(),
            )?
            .filter(
                move |value| {
                    value_should_be_printed(
                        value,
                        &filter_config,
                        &lower_terms,
                        span,
                        &cols_to_search_in_filter,
                        invert,
                    )
                },
                ctrlc,
            ),
        PipelineData::ListStream(stream, meta) => Ok(ListStream::from_stream(
            stream
                .map(move |mut x| {
                    let span = x.span();
                    match &mut x {
                        Value::Record { val, .. } => highlight_terms_in_record_with_search_columns(
                            &cols_to_search_in_map,
                            val,
                            span,
                            &config,
                            &terms,
                            string_style,
                            highlight_style,
                        ),
                        _ => x,
                    }
                })
                .filter(move |value| {
                    value_should_be_printed(
                        value,
                        &filter_config,
                        &lower_terms,
                        span,
                        &cols_to_search_in_filter,
                        invert,
                    )
                }),
            ctrlc.clone(),
        )
        .into_pipeline_data(ctrlc)
        .set_metadata(meta)),
        PipelineData::ExternalStream { stdout: None, .. } => Ok(PipelineData::empty()),
        PipelineData::ExternalStream {
            stdout: Some(stream),
            ..
        } => {
            let mut output: Vec<Value> = vec![];
            for filter_val in stream {
                match filter_val {
                    Ok(value) => {
                        let span = value.span();
                        match value {
                            Value::String { val, .. } => {
                                let split_char = if val.contains("\r\n") { "\r\n" } else { "\n" };

                                for line in val.split(split_char) {
                                    for term in lower_terms.iter() {
                                        let term_str = term.into_string("", &filter_config);
                                        let lower_val = line.to_lowercase();
                                        if lower_val
                                            .contains(&term.into_string("", &config).to_lowercase())
                                        {
                                            output.push(Value::string(
                                                highlight_search_string(
                                                    line,
                                                    &term_str,
                                                    &string_style,
                                                    &highlight_style,
                                                )?,
                                                span,
                                            ))
                                        }
                                    }
                                }
                            }
                            // Propagate errors by explicitly matching them before the final case.
                            Value::Error { error, .. } => return Err(*error),
                            other => {
                                return Err(ShellError::UnsupportedInput(
                                    "unsupported type from raw stream".into(),
                                    format!("input: {:?}", other.get_type()),
                                    span,
                                    // This line requires the Value::Error match above.
                                    other.span(),
                                ));
                            }
                        }
                    }
                    // Propagate any errors that were in the stream
                    Err(e) => return Err(e),
                };
            }
            Ok(output.into_pipeline_data(ctrlc))
        }
    }
}

fn value_should_be_printed(
    value: &Value,
    filter_config: &Config,
    lower_terms: &[Value],
    span: Span,
    columns_to_search: &[String],
    invert: bool,
) -> bool {
    let lower_value = Value::string(value.into_string("", filter_config).to_lowercase(), span);

    let mut match_found = lower_terms.iter().any(|term| match value {
        Value::Bool { .. }
        | Value::Int { .. }
        | Value::Filesize { .. }
        | Value::Duration { .. }
        | Value::Date { .. }
        | Value::Range { .. }
        | Value::Float { .. }
        | Value::Block { .. }
        | Value::Closure { .. }
        | Value::Nothing { .. }
        | Value::Error { .. } => term_equals_value(term, &lower_value, span),
        Value::String { .. }
        | Value::List { .. }
        | Value::CellPath { .. }
        | Value::CustomValue { .. } => term_contains_value(term, &lower_value, span),
        Value::Record { val, .. } => {
            record_matches_term(val, columns_to_search, filter_config, term, span)
        }
        Value::LazyRecord { val, .. } => match val.collect() {
            Ok(val) => match val {
                Value::Record { val, .. } => {
                    record_matches_term(&val, columns_to_search, filter_config, term, span)
                }
                _ => false,
            },
            Err(_) => false,
        },
        Value::Binary { .. } => false,
        Value::MatchPattern { .. } => false,
    });
    if invert {
        match_found = !match_found;
    }
    match_found
}

fn term_contains_value(term: &Value, value: &Value, span: Span) -> bool {
    term.r#in(span, value, span)
        .map_or(false, |value| value.is_true())
}

fn term_equals_value(term: &Value, value: &Value, span: Span) -> bool {
    term.eq(span, value, span)
        .map_or(false, |value| value.is_true())
}

fn record_matches_term(
    record: &Record,
    columns_to_search: &[String],
    filter_config: &Config,
    term: &Value,
    span: Span,
) -> bool {
    let cols_to_search = if columns_to_search.is_empty() {
        &record.cols
    } else {
        columns_to_search
    };
    record.iter().any(|(col, val)| {
        if !cols_to_search.contains(col) {
            return false;
        }
        let lower_val = if !val.is_error() {
            Value::string(
                val.into_string("", filter_config).to_lowercase(),
                Span::test_data(),
            )
        } else {
            (*val).clone()
        };
        term_contains_value(term, &lower_val, span)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_examples() {
        use crate::test_examples;

        test_examples(Find)
    }
}

fn split_string_if_multiline(input: PipelineData, head_span: Span) -> PipelineData {
    let span = input.span().unwrap_or(head_span);
    match input {
        PipelineData::Value(Value::String { ref val, .. }, _) => {
            if val.contains('\n') {
                Value::list(
                    val.lines()
                        .map(|s| Value::string(s.to_string(), span))
                        .collect(),
                    span,
                )
                .into_pipeline_data()
                .set_metadata(input.metadata())
            } else {
                input
            }
        }
        _ => input,
    }
}
