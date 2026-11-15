//! Pin the exported contract independently of the zbus wire-type implementation.
use super::*;
use std::collections::BTreeMap;

fn attribute<'a>(tag: &'a str, name: &str) -> &'a str {
    tag.split_once(&format!("{name}=\""))
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
}

async fn assert_contract(
    bus: &WireHarness,
    path: &str,
    interface: &str,
    methods: &[(&str, &str, &str)],
    properties: &[(&str, &str, &str)],
) {
    let xml: String = bus
        .proxy(path, "org.freedesktop.DBus.Introspectable")
        .await
        .call("Introspect", &())
        .await
        .unwrap();
    let body = xml
        .split_once(&format!("<interface name=\"{interface}\">"))
        .unwrap()
        .1
        .split_once("</interface>")
        .unwrap()
        .0;
    let actual_methods: BTreeMap<_, _> = body
        .split("<method ")
        .skip(1)
        .map(|method| {
            let (header, body) = method.split_once('>').unwrap();
            let body = body.split_once("</method>").unwrap().0;
            let mut input = String::new();
            let mut output = String::new();
            for arg in body.split("<arg ").skip(1) {
                let tag = arg.split_once('>').unwrap().0;
                match attribute(tag, "direction") {
                    "in" => input.push_str(attribute(tag, "type")),
                    "out" => output.push_str(attribute(tag, "type")),
                    _ => panic!("unexpected introspection argument direction"),
                }
            }
            (attribute(header, "name"), (input, output))
        })
        .collect();
    let expected_methods: BTreeMap<_, _> = methods
        .iter()
        .map(|(name, input, output)| (*name, (input.to_string(), output.to_string())))
        .collect();
    assert_eq!(actual_methods, expected_methods, "{interface} methods");
    let actual_properties: BTreeMap<_, _> = body
        .split("<property ")
        .skip(1)
        .map(|property| {
            let tag = property.split_once('>').unwrap().0;
            (
                attribute(tag, "name"),
                (attribute(tag, "type"), attribute(tag, "access")),
            )
        })
        .collect();
    let expected_properties: BTreeMap<_, _> = properties
        .iter()
        .map(|(name, signature, access)| (*name, (*signature, *access)))
        .collect();
    assert_eq!(
        actual_properties, expected_properties,
        "{interface} properties"
    );
}

#[tokio::test]
async fn exported_secret_service_signatures_and_plain_session_remain_compatible() {
    let store = Arc::new(RwLock::new(SecretStore::new_default()));
    let script = ScriptedPublisher::new(vec![HostResponse::Ack]);
    let bus = WireHarness::new(&store, &script.publisher()).await;
    let service = bus.proxy(BASE_PATH, SERVICE).await;
    let collection_path = collection_path("default").unwrap();
    let collection = bus.proxy(collection_path.as_str(), COLLECTION).await;
    let item_path = create_item(&collection).await;
    assert_contract(
        &bus,
        BASE_PATH,
        SERVICE,
        &[
            ("OpenSession", "sv", "vo"),
            ("CreateCollection", "a{sv}s", "oo"),
            ("SearchItems", "a{ss}", "aoao"),
            ("Unlock", "ao", "aoo"),
            ("Lock", "ao", "aoo"),
            ("GetSecrets", "aoo", "a{o(oayays)}"),
            ("ReadAlias", "s", "o"),
            ("SetAlias", "so", ""),
        ],
        &[("Collections", "ao", "read")],
    )
    .await;
    assert_contract(
        &bus,
        collection_path.as_str(),
        COLLECTION,
        &[
            ("Delete", "", "o"),
            ("SearchItems", "a{ss}", "ao"),
            ("CreateItem", "a{sv}(oayays)b", "oo"),
        ],
        &[
            ("Items", "ao", "read"),
            ("Label", "s", "readwrite"),
            ("Locked", "b", "read"),
            ("Created", "t", "read"),
            ("Modified", "t", "read"),
        ],
    )
    .await;
    assert_contract(
        &bus,
        item_path.as_str(),
        ITEM,
        &[
            ("Delete", "", "o"),
            ("GetSecret", "o", "(oayays)"),
            ("SetSecret", "(oayays)", ""),
        ],
        &[
            ("Locked", "b", "read"),
            ("Attributes", "a{ss}", "readwrite"),
            ("Label", "s", "readwrite"),
            ("Created", "t", "read"),
            ("Modified", "t", "read"),
        ],
    )
    .await;
    let input = OwnedVariant::from(Str::from(""));
    let (output, session): (OwnedVariant, OwnedObjectPath) = service
        .call("OpenSession", &("plain", &input))
        .await
        .unwrap();
    assert_eq!(<&str>::try_from(&output).unwrap(), "");
    assert_eq!(session.as_str(), "/org/freedesktop/secrets/session/s0");
    let (_, reused): (OwnedVariant, OwnedObjectPath) = service
        .call("OpenSession", &("plain", &input))
        .await
        .unwrap();
    assert_eq!(reused, session);
    assert_contract(
        &bus,
        session.as_str(),
        "org.freedesktop.Secret.Session",
        &[("Close", "", "")],
        &[],
    )
    .await;
    bus.proxy(session.as_str(), "org.freedesktop.Secret.Session")
        .await
        .call::<_, _, ()>("Close", &())
        .await
        .unwrap();
    let unsupported = service
        .call::<_, _, (OwnedVariant, OwnedObjectPath)>("OpenSession", &("unsupported", input))
        .await;
    assert!(matches!(
        unsupported,
        Err(zbus::Error::FDO(error)) if matches!(*error, zbus::fdo::Error::NotSupported(_))
    ));
    drop((service, collection));
    bus.close().await;
    assert_eq!(script.finish().len(), 1);
}

#[tokio::test]
async fn shared_property_setters_preserve_wire_errors_and_rejected_state() {
    let mut initial = SecretStore::new_default();
    let id = initial
        .create_item("default", "", &HashMap::new(), b"", "text/plain", false)
        .unwrap()
        .0;
    let before = initial.clone();
    let store = Arc::new(RwLock::new(initial));
    let script = ScriptedPublisher::new(vec![HostResponse::Reject; 3]);
    let bus = WireHarness::new(&store, &script.publisher()).await;
    let collection_path = collection_path("default").unwrap();
    let collection = bus.proxy(collection_path.as_str(), COLLECTION).await;
    let item_path = format!("{collection_path}/{id}");
    let item = bus.proxy(&item_path, ITEM).await;
    for (proxy, property) in [
        (&collection, "Label"),
        (&item, "Label"),
        (&item, "Attributes"),
    ] {
        assert!(matches!(
            proxy.set_property(property, 7u32).await,
            Err(zbus::fdo::Error::InvalidArgs(_))
        ));
        assert!(
            *store.read().await == before,
            "invalid property changed accepted state"
        );
        let result = if property == "Attributes" {
            proxy
                .set_property(property, HashMap::<String, String>::new())
                .await
        } else {
            proxy.set_property(property, "rejected_label").await
        };
        assert!(matches!(result, Err(zbus::fdo::Error::Failed(_))));
        assert!(
            *store.read().await == before,
            "rejected property changed accepted state"
        );
    }
    drop((collection, item));
    bus.close().await;
    assert_eq!(script.finish().len(), 3);
}
