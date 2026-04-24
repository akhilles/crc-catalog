use std::{
    collections::HashMap,
    error::Error,
    fmt,
    io::{self, BufWriter, Write},
    str::FromStr,
};
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

const CATALOG_URL: &str = "https://reveng.sourceforge.io/crc-catalogue/all.htm";

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn main() -> Result<()> {
    let agent = ureq::Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent();
    let mut response = agent.get(CATALOG_URL).call()?;
    if !response.status().is_success() {
        return Err(format!("GET {CATALOG_URL} failed: HTTP {}", response.status()).into());
    }
    let html = response.body_mut().read_to_string()?;
    let mut writer = BufWriter::new(io::stdout());

    writeln!(
        writer,
        r#"//! CRC algorithms as structs.
use crate::Algorithm;
"#
    )?;

    for algorithm in parse_catalog(&html)? {
        algorithm.emit_rust(&mut writer)?;
    }

    Ok(())
}

fn parse_catalog(html: &str) -> Result<Vec<Algorithm>> {
    let dom = tl::parse(html, tl::ParserOptions::default())?;
    let parser = dom.parser();

    let anchors = dom.nodes().iter().filter_map(|node| {
        let tag = node.as_tag()?;
        tag_name_eq(tag, "h3").then(|| {
            tag.children()
                .all(parser)
                .iter()
                .filter_map(|child| {
                    let child = child.as_tag()?;
                    tag_name_eq(child, "a")
                        .then(|| attr_value(child, "name"))
                        .flatten()
                })
                .next_back()
        })?
    });

    let parameters = dom.nodes().iter().filter_map(|node| {
        let tag = node.as_tag()?;
        tag_name_eq(tag, "code").then(|| {
            let text = node.inner_text(parser);
            text.trim_start()
                .starts_with("width=")
                .then(|| text.into_owned())
        })?
    });

    let algorithms = anchors
        .zip(parameters)
        .map(|(anchor, parameters)| {
            Ok(Algorithm {
                parameters: parameters.parse()?,
                url: format!("{CATALOG_URL}#{anchor}"),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    if algorithms.is_empty() {
        return Err("catalogue parse found no algorithms".into());
    }

    Ok(algorithms)
}

fn tag_name_eq(tag: &tl::HTMLTag<'_>, name: &str) -> bool {
    tag.name().as_utf8_str().eq_ignore_ascii_case(name)
}

fn attr_value(tag: &tl::HTMLTag<'_>, name: &str) -> Option<String> {
    let uppercase = name.to_ascii_uppercase();
    tag.attributes()
        .get(name)
        .or_else(|| tag.attributes().get(uppercase.as_str()))
        .and_then(|value| value)
        .map(|value| value.as_utf8_str().into_owned())
}

pub struct Algorithm {
    parameters: Parameters,
    url: String,
}

impl Algorithm {
    pub fn emit_rust(&self, mut writer: impl Write) -> Result<()> {
        let const_name = self.parameters.name.replace(['-', '/'], "_");
        let int_ty = int_type_for(self.parameters.width);
        let poly_rev = reverse_bits(self.parameters.poly, self.parameters.width);
        let Self {
            parameters:
                Parameters {
                    width,
                    poly,
                    init,
                    refin,
                    refout,
                    xorout,
                    check,
                    residue,
                    name,
                },
            url,
        } = &self;

        writeln!(
            writer,
            r#"/// # [`{name}`][1]
///
/// - `width`: `{width}` bits
/// - `poly`: `0x{poly:x}` (reversed: `0x{poly_rev:x}`)
/// - `init`: `0x{init:x}`
/// - `refin`: `{refin:?}`
/// - `refout`: `{refout:?}`
/// - `xorout`: `0x{xorout:x}`
/// - `check`: `0x{check:x}`
/// - `residue`: `0x{residue:x}`
///
/// [1]: {url}
pub const {const_name}: Algorithm<{int_ty}> = Algorithm {{
    width: {width},
    poly: 0x{poly:x},
    init: 0x{init:x},
    refin: {refin:?},
    refout: {refout:?},
    xorout: 0x{xorout:x},
    check: 0x{check:x},
    residue: 0x{residue:x}
}};
"#
        )?;

        Ok(())
    }
}

#[derive(Debug)]
struct ParseParametersError(String);

impl fmt::Display for ParseParametersError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for ParseParametersError {}

struct Parameters {
    width: u8,
    poly: u128,
    init: u128,
    refin: bool,
    refout: bool,
    xorout: u128,
    check: u128,
    residue: u128,
    name: String,
}

impl FromStr for Parameters {
    type Err = ParseParametersError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let fields = parse_fields(s)?;

        Ok(Self {
            width: parse_dec_field(&fields, "width")?,
            poly: parse_hex_field(&fields, "poly")?,
            init: parse_hex_field(&fields, "init")?,
            refin: parse_bool_field(&fields, "refin")?,
            refout: parse_bool_field(&fields, "refout")?,
            xorout: parse_hex_field(&fields, "xorout")?,
            check: parse_hex_field(&fields, "check")?,
            residue: parse_hex_field(&fields, "residue")?,
            name: field_value(&fields, "name")?.to_owned(),
        })
    }
}

fn parse_fields(s: &str) -> std::result::Result<HashMap<String, String>, ParseParametersError> {
    shlex::split(s)
        .ok_or_else(|| ParseParametersError("invalid quoted parameter string".into()))?
        .into_iter()
        .map(|field| {
            let (name, value) = field
                .split_once('=')
                .ok_or_else(|| ParseParametersError(format!("invalid field: {field}")))?;
            Ok((name.to_owned(), value.to_owned()))
        })
        .collect()
}

fn parse_dec_field<T>(
    fields: &HashMap<String, String>,
    field: &str,
) -> std::result::Result<T, ParseParametersError>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    field_value(fields, field)?
        .parse()
        .map_err(|err| ParseParametersError(format!("invalid {field}: {err}")))
}

fn parse_hex_field(
    fields: &HashMap<String, String>,
    field: &str,
) -> std::result::Result<u128, ParseParametersError> {
    let value = field_value(fields, field)?;
    let value = value
        .strip_prefix("0x")
        .ok_or_else(|| ParseParametersError(format!("{field} is missing 0x prefix")))?;
    u128::from_str_radix(value, 16)
        .map_err(|err| ParseParametersError(format!("invalid {field}: {err}")))
}

fn parse_bool_field(
    fields: &HashMap<String, String>,
    field: &str,
) -> std::result::Result<bool, ParseParametersError> {
    match field_value(fields, field)? {
        "false" => Ok(false),
        "true" => Ok(true),
        value => Err(ParseParametersError(format!(
            "invalid {field}: expected true or false, got {value}"
        ))),
    }
}

fn field_value<'a>(
    fields: &'a HashMap<String, String>,
    field: &str,
) -> std::result::Result<&'a str, ParseParametersError> {
    fields
        .get(field)
        .map(String::as_str)
        .ok_or_else(|| ParseParametersError(format!("missing {field} field")))
}

fn int_type_for(width: u8) -> &'static str {
    let width = u32::from(width);
    macro_rules! int_tys {
        ($($ty:ident),*) => {
            $(
                if width <= $ty::BITS {
                    return stringify!($ty);
                }
            )*
        };
    }
    int_tys!(u8, u16, u32, u64, u128);
    panic!("No unsigned int type for width: {width}");
}

fn reverse_bits(x: u128, width: u8) -> u128 {
    x.reverse_bits() >> (128 - width)
}

#[cfg(test)]
mod tests {
    use crate::Parameters;

    #[test]
    fn it_parses_parameters() {
        let s = "width=3  poly=0x3  init=0x0  refin=false  refout=false  xorout=0x7  check=0x4  residue=0x2  name=\"CRC-3/GSM\"";
        let params: Parameters = s.parse().unwrap();
        assert_eq!(params.width, 3);
        assert_eq!(params.name, "CRC-3/GSM");
    }
}
