use crate::input_validation::*;

#[test]
fn values_a_shell_or_a_tool_would_reinterpret_are_rejected() {
    for value in [
        "", "-avd", "--help", "a$b", "${HOME}", "%PATH%", "a%b", "a;b", "a b", "a|b", "a`b", "a\nb",
    ] {
        let error =
            validate_argument("value", value).expect_err(&format!("{value:?} must be rejected"));
        assert_eq!(error.value, value);
        assert!(!error.reason.is_empty(), "{value:?} needs a reason");
    }
    for value in ["Medium_Phone", "emulator-5554", "a.b:c-d_e"] {
        assert_eq!(validate_argument("value", value), Ok(()), "{value:?}");
    }
}

#[test]
fn the_rejection_names_the_value_and_the_reason() {
    let error = validate_argument("AVD name", "-wipe-data").expect_err("leading dash");
    let message = error.to_string();
    assert!(message.contains("AVD name"), "{message}");
    assert!(message.contains("-wipe-data"), "{message}");
    assert!(message.contains(&error.reason), "{message}");
    assert!(!error.reason.is_empty());
}

#[test]
fn udids_of_simulators_and_physical_iphones_are_accepted() {
    for udid in [
        "00000000-0000-4000-8000-000000000001",
        "F2100A3B-1C2D-4E5F-8A9B-0C1D2E3F4A5B",
        "00008120-0000000000000001",
    ] {
        assert_eq!(validate_udid(udid), Ok(()), "{udid}");
    }
}

#[test]
fn udids_outside_the_hex_and_dash_shape_are_rejected() {
    for udid in [
        "",
        "-0000000-0000-4000-8000-000000000001",
        "short-1234",
        "G0000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-00000000000$",
        "00000000-0000-4000-8000-000000000001-00000000",
        "00000000 0000-4000-8000-000000000001",
    ] {
        assert!(validate_udid(udid).is_err(), "{udid:?} must be rejected");
    }
}

#[test]
fn serials_of_usb_emulator_and_wireless_devices_are_accepted() {
    for serial in [
        "emulator-5554",
        "R58M123ABC",
        "192.168.1.5:5555",
        "adb-R58M123ABC-abc123._adb-tls-connect._tcp",
    ] {
        assert_eq!(validate_serial(serial), Ok(()), "{serial}");
    }
    for serial in ["", "-s", "_leading", "a b", "a$b", "a%b", "a/b", "a;b"] {
        assert!(
            validate_serial(serial).is_err(),
            "{serial:?} must be rejected"
        );
    }
}

#[test]
fn avd_names_allow_letters_digits_dot_dash_and_underscore_only() {
    for name in ["Medium_Phone", "Pixel_7_API_35", "tablet-1.2"] {
        assert_eq!(validate_avd_name(name), Ok(()), "{name}");
    }
    for name in ["", "-avd", "_hidden", "Pixel 7", "a:b", "a$b", "a%b", "a/b"] {
        assert!(
            validate_avd_name(name).is_err(),
            "{name:?} must be rejected"
        );
    }
}

#[test]
fn wireless_addresses_must_be_private_or_link_local_literals_with_a_port() {
    for address in [
        "192.168.1.5:5555",
        "10.0.0.2:37099",
        "172.16.4.9:5555",
        "172.31.255.255:65535",
        "169.254.10.20:1",
        "[fe80::1]:5555",
    ] {
        let parsed = parse_wireless_address(address)
            .unwrap_or_else(|error| panic!("{address} must be accepted: {error}"));
        assert_eq!(parsed.to_string(), address);
    }
    for address in [
        "8.8.8.8:5555",
        "172.32.0.1:5555",
        "127.0.0.1:5555",
        "[2001:db8::1]:5555",
        "[::1]:5555",
        "phone.local:5555",
        "example.com:5555",
        "192.168.1.5",
        "192.168.1.5:0",
        "192.168.1.5:65536",
        "-192.168.1.5:5555",
        "[fe80::1%en0]:5555",
        "",
    ] {
        assert!(
            parse_wireless_address(address).is_err(),
            "{address:?} must be rejected"
        );
    }
}

#[test]
fn pairing_codes_are_exactly_six_digits_and_never_echoed() {
    assert!(parse_pairing_code("123456").is_ok());
    for code in [
        "",
        "12345",
        "1234567",
        "12345a",
        "-12345",
        "12 456",
        "١٢٣٤٥٦",
    ] {
        let error = parse_pairing_code(code).expect_err(&format!("{code:?} must be rejected"));
        if !code.is_empty() {
            assert!(
                !error.to_string().contains(code),
                "the message must not repeat the code: {error}"
            );
        }
    }
    let code = parse_pairing_code("654321").expect("valid code");
    assert!(!format!("{code:?}").contains("654321"));
}
