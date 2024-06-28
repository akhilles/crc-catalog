use std::{borrow::Cow, fs::File, io::{BufWriter, Write}, path::PathBuf, str::FromStr};

use color_eyre::eyre::{eyre, Error, OptionExt};
use ego_tree::iter::NextSiblings;
use lazy_static::lazy_static;
use pico_args::Arguments;
use scraper::{ElementRef, Html, Node, Selector};
use regex::Regex;

pub const CATALOG_URL: &'static str = "https://reveng.sourceforge.io/crc-catalogue/all.htm";

#[derive(Debug)]
struct Args {
    url: Option<String>,
    output: Option<PathBuf>,
}

impl Args {
    pub fn from_env() -> Result<Option<Self>, Error> {
        let mut args = Arguments::from_env();
        if args.contains(["-h", "--help"]) {
            println!("USAGE: {} [-o FILE] [URL]", std::env::args().next().map(Cow::Owned).unwrap_or_else(|| "generate-catalog".into()));
            return Ok(None);
        }

        let output = args.opt_value_from_str(["-o", "--output"])?;
        let url = args.opt_free_from_str()?;

        Ok(Some(Self {
            url,
            output,
        }))
    }
}

fn main() -> Result<(), Error> {
    let Some(args) = Args::from_env()? else { return Ok(()); };

    let writer = if let Some(output) = &args.output {
        Box::new(File::create(output)?) as Box<dyn Write>
    }
    else {
        Box::new(std::io::stdout())
    };
    let mut writer = BufWriter::new(writer);

    let url = args.url.as_deref().unwrap_or(CATALOG_URL);
    let html = ureq::get(url).call()?.into_string()?;
    let html = Html::parse_document(&html);

    let h3_selector = Selector::parse("h3").unwrap();
    let code_selector = Selector::parse("code").unwrap();

    writeln!(
        writer,
        r#"//! CRC algorithms as structs.
use crate::Algorithm;
"#)?;

    for h3 in html.select(&h3_selector) {
        let h3_a = ElementRef::wrap(h3.last_child().unwrap()).unwrap();
        let a_name = h3_a.attr("name").unwrap();
        let _name = h3_a.inner_html();

        let mut siblings = h3.next_siblings();
        let p = next_tag(&mut siblings);
        let Some(parameters) = p.select(&code_selector).next().and_then(|node| node.inner_html().parse().ok()) else { break; };

        let _ul = next_tag(&mut siblings);
        
        Algorithm {
            parameters,
            url: format!("{url}#{a_name}"),
            aliases: vec![],
        }.emit_rust(&mut writer)?;
    }

    Ok(())
}

fn next_tag<'a>(it: &'a mut NextSiblings<Node>) -> ElementRef<'a> {
    loop {
        let item = it.next().expect("Expected a sibling");
        match item.value() {
            Node::Element(_) => return ElementRef::wrap(item).unwrap(),
            _ => {}
        }
    }
}

pub struct Algorithm {
    parameters: Parameters,
    url: String,
    aliases: Vec<String>,
}

impl Algorithm {
    pub fn emit_rust(&self, mut writer: impl Write) -> Result<(), Error> {
        let const_name = NAME_REPLACE_REGEX.replace_all(&self.parameters.name, "_");
        let int_ty = int_type_for(self.parameters.width);
        let poly_rev = reverse_bits(self.parameters.poly, self.parameters.width);
        let Self {
            parameters: Parameters { width, poly, init, refin, refout, xorout, check, residue, name },
            url,
            aliases,
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
"#)?;

        for alias in aliases {
            writeln!(
                writer,
                r#"/// Alias for [`{const_name}`].
pub const {alias}: Algorithm<{int_ty}> = {const_name};
"#
            )?;
        }

        Ok(())
    }
}


lazy_static! {
    static ref PARAMETERS_REGEX: Regex = r#"width=(\d+)\s+poly=0x([0-9a-fA-F]+)\s+init=0x([0-9a-fA-F]+)\s+refin=(false|true)\s+refout=(false|true)\s+xorout=0x([0-9a-fA-F]+)\s+check=0x([0-9a-fA-F]+)\s+residue=0x([0-9a-fA-F]+)\s+name="([^"]+)""#.parse().unwrap();
    static ref NAME_REPLACE_REGEX: Regex = r"[-/]".parse().unwrap();
}

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
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (_, params) = PARAMETERS_REGEX.captures(s).ok_or_eyre("Parameters regex didn't match")?.extract::<9>();
        Ok(Self {
            width: params[0].parse()?,
            poly: u128::from_str_radix(params[1], 16)?,
            init: u128::from_str_radix(params[2], 16)?,
            refin: parse_bool(params[3])?,
            refout: parse_bool(params[4])?,
            xorout: u128::from_str_radix(params[5], 16)?,
            check: u128::from_str_radix(params[6], 16)?,
            residue: u128::from_str_radix(params[7], 16)?,
            name: params[8].to_owned(),
        })
    }
}

fn parse_bool(s: &str) -> Result<bool, Error> {
    if s.eq_ignore_ascii_case("false") {
        Ok(false)
    }
    else if s.eq_ignore_ascii_case("true") {
        Ok(true)
    }
    else {
        Err(eyre!("Not a valid boolean: {s}"))
    }
}

fn int_type_for(width: u8) -> &'static str {
    let width: u32 = width.try_into().unwrap();
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
        let _params: Parameters = s.parse().unwrap();
    }
}