use super::*;
use std::io;
use tracing_subscriber::fmt::MakeWriter;

const LABEL_ERROR: &str = "org.freedesktop.Secret.Item.Label must have D-Bus type s";
const ATTRIBUTES_ERROR: &str = "org.freedesktop.Secret.Item.Attributes must have D-Bus type a{ss}";
const CONTROL_TRACE_EVENT: &str = "f046_trace_capture_control";

fn invalid_args_message<T>(result: fdo::Result<T>) -> String {
    match result {
        Err(fdo::Error::InvalidArgs(message)) => message,
        Err(error) => panic!("expected InvalidArgs, got {error}"),
        Ok(_) => panic!("expected InvalidArgs, got success"),
    }
}

#[derive(Clone, Default)]
struct TracingWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for TracingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for TracingWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn assert_trace_capture_active(writer: &TracingWriter) {
    tracing::info!(control = CONTROL_TRACE_EVENT, "F-046 trace capture control");
    let captured = String::from_utf8_lossy(&writer.0.lock().unwrap()).into_owned();
    assert!(captured.contains(CONTROL_TRACE_EVENT));
}

fn assert_metadata_trace_enabled_when_requested() {
    if std::env::var_os("UNDERWRAP_DEBUG_SECRETSD_TRACE_METADATA").is_some() {
        assert!(metadata_trace_enabled());
    }
}

#[test]
fn item_property_parsers_accept_absent_and_exact_types() {
    assert_eq!(parse_item_label(&HashMap::new()).unwrap(), "");
    for label in ["", "nonempty-label"] {
        let properties = HashMap::from([(
            ITEM_LABEL_PROPERTY.to_string(),
            Value::from(label.to_string()),
        )]);
        assert_eq!(parse_item_label(&properties).unwrap(), label);
    }

    assert_eq!(
        parse_item_attributes(&HashMap::new()).unwrap(),
        HashMap::new()
    );
    for entries in [&[][..], &[("key", "value")][..]] {
        let properties =
            HashMap::from([(ITEM_ATTRIBUTES_PROPERTY.to_string(), string_dict(entries))]);
        assert_eq!(
            parse_item_attributes(&properties).unwrap(),
            entries
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect()
        );
    }
}

#[test]
fn item_property_parsers_reject_wrong_outer_and_dictionary_types() {
    let wrong_labels = [
        Value::from(7u32),
        string_dict(&[("container", "value")]),
        Value::Value(Box::new(Value::from("nested-label".to_string()))),
    ];
    for label in wrong_labels {
        let properties = HashMap::from([(ITEM_LABEL_PROPERTY.to_string(), label)]);
        assert_eq!(
            invalid_args_message(parse_item_label(&properties)),
            LABEL_ERROR
        );
    }

    let wrong_attributes = [
        Value::from("not-a-dictionary".to_string()),
        Value::Value(Box::new(string_dict(&[("nested", "dictionary")]))),
        dict_value(
            <u32 as Type>::signature(),
            <String as Type>::signature(),
            vec![(Value::from(1u32), Value::from("value".to_string()))],
        ),
        dict_value(
            <String as Type>::signature(),
            <u32 as Type>::signature(),
            vec![(Value::from("key".to_string()), Value::from(1u32))],
        ),
        string_variant_dict(&[("key", "string-payload")]),
    ];
    for attributes in wrong_attributes {
        let properties = HashMap::from([(ITEM_ATTRIBUTES_PROPERTY.to_string(), attributes)]);
        assert_eq!(
            invalid_args_message(parse_item_attributes(&properties)),
            ATTRIBUTES_ERROR
        );
    }
}

#[test]
fn create_item_source_checks_outer_signatures_in_static_prevalidation_order() {
    let source = include_str!("../../collection.rs");
    let label_parser = &source[source.find("fn parse_item_label").unwrap()
        ..source.find("fn parse_item_attributes").unwrap()];
    assert!(
        label_parser.find("label.value_signature()").unwrap()
            < label_parser.find("downcast_ref::<String>()").unwrap()
    );
    let attributes_parser = &source[source.find("fn parse_item_attributes").unwrap()
        ..source.find("pub struct CollectionInterface").unwrap()];
    assert!(
        attributes_parser
            .find("attributes.value_signature()")
            .unwrap()
            < attributes_parser.find("downcast_ref").unwrap()
    );
    assert!(
        attributes_parser.find("downcast_ref").unwrap()
            < attributes_parser
                .find("HashMap::<String, String>::try_from")
                .unwrap()
    );

    let handler = &source[source.find("async fn create_item").unwrap()
        ..source
            .find("#[zbus(property)]\n    async fn items")
            .unwrap()];
    let label_parse = handler.find("parse_item_label(&properties)").unwrap();
    let attributes_parse = handler.find("parse_item_attributes(&properties)").unwrap();
    assert!(label_parse < attributes_parse);
    for marker in [
        "metadata_trace_enabled()",
        "tracing::info!",
        "self.store.write()",
        "mutate_and_publish",
        ".create_item(",
        "object_server.at",
    ] {
        let marker_position = handler.find(marker).unwrap();
        assert!(
            label_parse < marker_position,
            "Label parser must precede {marker}"
        );
        assert!(
            attributes_parse < marker_position,
            "Attributes parser must precede {marker}"
        );
    }
    assert!(
        handler.find("metadata_trace_enabled()").unwrap() < handler.find("tracing::info!").unwrap()
    );
    let mutation_order = [
        "self.store.write()",
        "mutate_and_publish",
        ".create_item(",
        "object_server.at",
    ];
    for pair in mutation_order.windows(2) {
        assert!(
            handler.find(pair[0]).unwrap() < handler.find(pair[1]).unwrap(),
            "{} must precede {}",
            pair[0],
            pair[1]
        );
    }
}

#[derive(Clone, Copy, Debug)]
enum MalformedProperties {
    WrongLabel,
    NonDictAttributes,
    VariantAttributes,
    BothWrong,
}

fn malformed_properties(
    row: MalformedProperties,
    property_sentinel: &str,
    attribute_sentinel: &str,
) -> (HashMap<String, Value<'static>>, &'static str) {
    let label = Value::from(property_sentinel.to_string());
    let valid_attributes = string_dict(&[("match", "value"), ("sentinel", attribute_sentinel)]);
    let entries = match row {
        MalformedProperties::WrongLabel => [
            (
                ITEM_LABEL_PROPERTY.to_string(),
                Value::Value(Box::new(label)),
            ),
            (ITEM_ATTRIBUTES_PROPERTY.to_string(), valid_attributes),
        ],
        MalformedProperties::NonDictAttributes => [
            (ITEM_LABEL_PROPERTY.to_string(), label),
            (
                ITEM_ATTRIBUTES_PROPERTY.to_string(),
                Value::from(attribute_sentinel.to_string()),
            ),
        ],
        MalformedProperties::VariantAttributes => [
            (ITEM_LABEL_PROPERTY.to_string(), label),
            (
                ITEM_ATTRIBUTES_PROPERTY.to_string(),
                string_variant_dict(&[("sentinel", attribute_sentinel)]),
            ),
        ],
        MalformedProperties::BothWrong => [
            (
                ITEM_LABEL_PROPERTY.to_string(),
                string_dict(&[("sentinel", property_sentinel)]),
            ),
            (
                ITEM_ATTRIBUTES_PROPERTY.to_string(),
                Value::Value(Box::new(string_dict(&[("sentinel", attribute_sentinel)]))),
            ),
        ],
    };
    let expected = match row {
        MalformedProperties::WrongLabel | MalformedProperties::BothWrong => LABEL_ERROR,
        MalformedProperties::NonDictAttributes | MalformedProperties::VariantAttributes => {
            ATTRIBUTES_ERROR
        }
    };
    (HashMap::from(entries), expected)
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_item_properties_fail_before_state_publication_or_registration() {
    for replace in [false, true] {
        for row in [
            MalformedProperties::WrongLabel,
            MalformedProperties::NonDictAttributes,
            MalformedProperties::VariantAttributes,
            MalformedProperties::BothWrong,
        ] {
            let case = format!("{row:?}-{replace}");
            let property_sentinel = format!("property-sentinel-{case}");
            let attribute_sentinel = format!("attribute-sentinel-{case}");
            let accepted_sentinel = format!("accepted-secret-sentinel-{case}");
            let request_sentinel = format!("request-secret-sentinel-{case}");
            let mut initial = store_with_collection();
            let existing_attributes = HashMap::from([
                ("match".into(), "value".into()),
                ("sentinel".into(), attribute_sentinel.clone()),
            ]);
            let (item_id, _) = initial
                .create_item(
                    "Test",
                    "existing-label",
                    &existing_attributes,
                    accepted_sentinel.as_bytes(),
                    "text/existing",
                    false,
                )
                .unwrap();
            let before = initial.clone();
            let (service, store, script) = service_with_script(initial, vec![]);
            let collection =
                CollectionInterface::new("Test".into(), store.clone(), script.publisher());
            let object_server = ObjectServerHarness::new().await;
            let existing_path = format!("{BASE_PATH}/collection/Test/{item_id}");
            if replace {
                let server = object_server.object_server();
                server
                    .at(
                        existing_path.as_str(),
                        ItemInterface::new(
                            item_id,
                            "Test".into(),
                            store.clone(),
                            script.publisher(),
                        ),
                    )
                    .await
                    .unwrap();
            }
            let (properties, expected) =
                malformed_properties(row, &property_sentinel, &attribute_sentinel);
            let log_writer = TracingWriter::default();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(log_writer.clone())
                .with_ansi(false)
                .with_target(false)
                .finish();
            let _guard = tracing::subscriber::set_default(subscriber);
            assert_trace_capture_active(&log_writer);
            assert_metadata_trace_enabled_when_requested();
            let result = {
                let server = object_server.object_server();
                collection
                    .create_item(
                        properties,
                        secret(request_sentinel.as_bytes()),
                        replace,
                        &server,
                    )
                    .await
            };
            let diagnostics = format!(
                "{result:?}\n{}",
                String::from_utf8_lossy(&log_writer.0.lock().unwrap())
            );
            assert_eq!(invalid_args_message(result), expected, "{case}");
            assert_eq!(*store.read().await, before, "{case}");
            assert!(script.finish().is_empty(), "{case}");
            if replace {
                assert!(object_server.has_item(&existing_path).await, "{case}");
            }
            for sentinel in [
                property_sentinel.as_str(),
                attribute_sentinel.as_str(),
                accepted_sentinel.as_str(),
                request_sentinel.as_str(),
            ] {
                assert!(!diagnostics.contains(sentinel), "{case}: leaked {sentinel}");
            }
            object_server.close().await;
            drop(service);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_item_property_precedes_missing_collection_lookup() {
    let accepted_sentinel = "missing-collection-accepted-secret-sentinel";
    let request_sentinel = "missing-collection-request-secret-sentinel";
    let property_sentinel = "missing-collection-property-sentinel";
    let attribute_sentinel = "missing-collection-attribute-sentinel";
    let mut initial = SecretStore::new_default();
    initial
        .create_item(
            "default",
            "sentinel",
            &HashMap::new(),
            accepted_sentinel.as_bytes(),
            "text/plain",
            false,
        )
        .unwrap();
    let before = initial.clone();
    let (service, store, script) = service_with_script(initial, vec![]);
    let collection = CollectionInterface::new("Missing".into(), store.clone(), script.publisher());
    let object_server = ObjectServerHarness::new().await;
    let properties = HashMap::from([
        (
            ITEM_LABEL_PROPERTY.to_string(),
            Value::Value(Box::new(Value::from(property_sentinel.to_string()))),
        ),
        (
            ITEM_ATTRIBUTES_PROPERTY.to_string(),
            string_dict(&[("sentinel", attribute_sentinel)]),
        ),
    ]);

    let log_writer = TracingWriter::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(log_writer.clone())
        .with_ansi(false)
        .with_target(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    assert_trace_capture_active(&log_writer);
    assert_metadata_trace_enabled_when_requested();
    let result = {
        let server = object_server.object_server();
        collection
            .create_item(
                properties,
                secret(request_sentinel.as_bytes()),
                false,
                &server,
            )
            .await
    };
    let diagnostics = format!(
        "{result:?}\n{}",
        String::from_utf8_lossy(&log_writer.0.lock().unwrap())
    );

    assert_eq!(invalid_args_message(result), LABEL_ERROR);
    assert_eq!(*store.read().await, before);
    assert!(script.finish().is_empty());
    for sentinel in [
        accepted_sentinel,
        request_sentinel,
        property_sentinel,
        attribute_sentinel,
    ] {
        assert!(!diagnostics.contains(sentinel));
    }
    object_server.close().await;
    drop(service);
}
