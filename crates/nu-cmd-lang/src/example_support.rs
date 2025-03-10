use itertools::Itertools;
use nu_protocol::{
    ast::{Block, RangeInclusion},
    engine::{EngineState, Stack, StateDelta, StateWorkingSet},
    Example, PipelineData, Signature, Span, Type, Value,
};
use std::collections::HashSet;

pub fn check_example_input_and_output_types_match_command_signature(
    example: &Example,
    cwd: &std::path::Path,
    engine_state: &mut Box<EngineState>,
    signature_input_output_types: &[(Type, Type)],
    signature_operates_on_cell_paths: bool,
) -> HashSet<(Type, Type)> {
    let mut witnessed_type_transformations = HashSet::<(Type, Type)>::new();

    // Skip tests that don't have results to compare to
    if let Some(example_output) = example.result.as_ref() {
        if let Some(example_input_type) =
            eval_pipeline_without_terminal_expression(example.example, cwd, engine_state)
        {
            let example_input_type = example_input_type.get_type();
            let example_output_type = example_output.get_type();

            let example_matches_signature =
                signature_input_output_types
                    .iter()
                    .any(|(sig_in_type, sig_out_type)| {
                        example_input_type.is_subtype(sig_in_type)
                            && example_output_type.is_subtype(sig_out_type)
                            && {
                                witnessed_type_transformations
                                    .insert((sig_in_type.clone(), sig_out_type.clone()));
                                true
                            }
                    });

            // The example type checks as a cell path operation if both:
            // 1. The command is declared to operate on cell paths.
            // 2. The example_input_type is list or record or table, and the example
            //    output shape is the same as the input shape.
            let example_matches_signature_via_cell_path_operation = signature_operates_on_cell_paths
                       && example_input_type.accepts_cell_paths()
                       // TODO: This is too permissive; it should make use of the signature.input_output_types at least.
                       && example_output_type.to_shape() == example_input_type.to_shape();

            if !(example_matches_signature || example_matches_signature_via_cell_path_operation) {
                panic!(
                    "The example `{}` demonstrates a transformation of type {:?} -> {:?}. \
                       However, this does not match the declared signature: {:?}.{} \
                       For this command `operates_on_cell_paths()` is {}.",
                    example.example,
                    example_input_type,
                    example_output_type,
                    signature_input_output_types,
                    if signature_input_output_types.is_empty() {
                        " (Did you forget to declare the input and output types for the command?)"
                    } else {
                        ""
                    },
                    signature_operates_on_cell_paths
                );
            };
        };
    }
    witnessed_type_transformations
}

fn eval_pipeline_without_terminal_expression(
    src: &str,
    cwd: &std::path::Path,
    engine_state: &mut Box<EngineState>,
) -> Option<Value> {
    let (mut block, delta) = parse(src, engine_state);
    if block.pipelines.len() == 1 {
        let n_expressions = block.pipelines[0].elements.len();
        block.pipelines[0].elements.truncate(&n_expressions - 1);

        if !block.pipelines[0].elements.is_empty() {
            let empty_input = PipelineData::empty();
            Some(eval_block(block, empty_input, cwd, engine_state, delta))
        } else {
            Some(Value::nothing(Span::test_data()))
        }
    } else {
        // E.g. multiple semicolon-separated statements
        None
    }
}

pub fn parse(contents: &str, engine_state: &EngineState) -> (Block, StateDelta) {
    let mut working_set = StateWorkingSet::new(engine_state);
    let output = nu_parser::parse(&mut working_set, None, contents.as_bytes(), false);

    if let Some(err) = working_set.parse_errors.first() {
        panic!("test parse error in `{contents}`: {err:?}")
    }

    (output, working_set.render())
}

pub fn eval_block(
    block: Block,
    input: PipelineData,
    cwd: &std::path::Path,
    engine_state: &mut Box<EngineState>,
    delta: StateDelta,
) -> Value {
    engine_state
        .merge_delta(delta)
        .expect("Error merging delta");

    let mut stack = Stack::new();

    stack.add_env_var("PWD".to_string(), Value::test_string(cwd.to_string_lossy()));

    match nu_engine::eval_block(engine_state, &mut stack, &block, input, true, true) {
        Err(err) => panic!("test eval error in `{}`: {:?}", "TODO", err),
        Ok(result) => result.into_value(Span::test_data()),
    }
}

pub fn check_example_evaluates_to_expected_output(
    example: &Example,
    cwd: &std::path::Path,
    engine_state: &mut Box<EngineState>,
) {
    let mut stack = Stack::new();

    // Set up PWD
    stack.add_env_var("PWD".to_string(), Value::test_string(cwd.to_string_lossy()));

    engine_state
        .merge_env(&mut stack, cwd)
        .expect("Error merging environment");

    let empty_input = PipelineData::empty();
    let result = eval(example.example, empty_input, cwd, engine_state);

    // Note. Value implements PartialEq for Bool, Int, Float, String and Block
    // If the command you are testing requires to compare another case, then
    // you need to define its equality in the Value struct
    if let Some(expected) = example.result.as_ref() {
        assert_eq!(
            DebuggableValue(&result),
            DebuggableValue(expected),
            "The example result differs from the expected value",
        )
    }
}

pub fn check_all_signature_input_output_types_entries_have_examples(
    signature: Signature,
    witnessed_type_transformations: HashSet<(Type, Type)>,
) {
    let declared_type_transformations = HashSet::from_iter(signature.input_output_types);
    assert!(
        witnessed_type_transformations.is_subset(&declared_type_transformations),
        "This should not be possible (bug in test): the type transformations \
        collected in the course of matching examples to the signature type map \
        contain type transformations not present in the signature type map."
    );

    if !signature.allow_variants_without_examples {
        assert_eq!(
            witnessed_type_transformations,
            declared_type_transformations,
            "There are entries in the signature type map which do not correspond to any example: \
            {:?}",
            declared_type_transformations
                .difference(&witnessed_type_transformations)
                .map(|(s1, s2)| format!("{s1} -> {s2}"))
                .join(", ")
        );
    }
}

fn eval(
    contents: &str,
    input: PipelineData,
    cwd: &std::path::Path,
    engine_state: &mut Box<EngineState>,
) -> Value {
    let (block, delta) = parse(contents, engine_state);
    eval_block(block, input, cwd, engine_state, delta)
}

pub struct DebuggableValue<'a>(pub &'a Value);

impl PartialEq for DebuggableValue<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<'a> std::fmt::Debug for DebuggableValue<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Value::Bool { val, .. } => {
                write!(f, "{:?}", val)
            }
            Value::Int { val, .. } => {
                write!(f, "{:?}", val)
            }
            Value::Float { val, .. } => {
                write!(f, "{:?}f", val)
            }
            Value::Filesize { val, .. } => {
                write!(f, "Filesize({:?})", val)
            }
            Value::Duration { val, .. } => {
                let duration = std::time::Duration::from_nanos(*val as u64);
                write!(f, "Duration({:?})", duration)
            }
            Value::Date { val, .. } => {
                write!(f, "Date({:?})", val)
            }
            Value::Range { val, .. } => match val.inclusion {
                RangeInclusion::Inclusive => write!(
                    f,
                    "Range({:?}..{:?}, step: {:?})",
                    val.from, val.to, val.incr
                ),
                RangeInclusion::RightExclusive => write!(
                    f,
                    "Range({:?}..<{:?}, step: {:?})",
                    val.from, val.to, val.incr
                ),
            },
            Value::String { val, .. } => {
                write!(f, "{:?}", val)
            }
            Value::Record { val, .. } => {
                write!(f, "{{")?;
                for i in 0..val.len() {
                    let col = &val.cols[i];
                    let value = &val.vals[i];

                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{:?}: {:?}", col, DebuggableValue(value))?;
                }
                write!(f, "}}")
            }
            Value::List { vals, .. } => {
                write!(f, "[")?;
                for (i, value) in vals.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{:?}", DebuggableValue(value))?;
                }
                write!(f, "]")
            }
            Value::Block { val, .. } => {
                write!(f, "Block({:?})", val)
            }
            Value::Closure { val, .. } => {
                write!(f, "Closure({:?})", val)
            }
            Value::Nothing { .. } => {
                write!(f, "Nothing")
            }
            Value::Error { error, .. } => {
                write!(f, "Error({:?})", error)
            }
            Value::Binary { val, .. } => {
                write!(f, "Binary({:?})", val)
            }
            Value::CellPath { val, .. } => {
                write!(f, "CellPath({:?})", val.into_string())
            }
            Value::CustomValue { val, .. } => {
                write!(f, "CustomValue({:?})", val)
            }
            Value::LazyRecord { val, .. } => {
                let rec = val.collect().map_err(|_| std::fmt::Error)?;
                write!(f, "LazyRecord({:?})", DebuggableValue(&rec))
            }
            Value::MatchPattern { val, .. } => {
                write!(f, "MatchPattern({:?})", val)
            }
        }
    }
}
