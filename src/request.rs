use nom::branch::alt;
use nom::bytes::complete::{is_a, tag, take_while1};
use nom::character::complete::not_line_ending;
use nom::combinator::{all_consuming, cond, eof, map, opt, recognize, value};
use nom::error::ErrorKind;
use nom::error_position;
use nom::sequence::{preceded, tuple};

#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// Whether the verbose "\W" flag is set or not
    pub verbose: bool,

    /// The user that was queried, if any
    ///
    /// If no user was given, this finger request should be treated as a user list request.
    pub user: Option<&'a str>,

    /// The part after the optional `@` sign, used for forwarding finger requests
    pub forwarding: Option<&'a str>,
}

impl<'a> Request<'a> {
    /// Create a request that just lists the users on the server
    pub fn new_list(verbose: bool) -> Self {
        Self {
            verbose,
            user: None,
            forwarding: None,
        }
    }

    pub fn from_str(input: &'a str) -> Result<Self, nom::Err<nom::error::Error<&'a str>>> {
        let Some(input) = input.strip_suffix("\r\n") else {
            let err = error_position!(&input[input.len()..], ErrorKind::Eof);
            return Err(nom::Err::Error(err));
        };

        match parse(input) {
            Ok(("", req)) => Ok(req),
            Ok(_) => unreachable!(),
            Err(err) => Err(err),
        }
    }
}

type IResult<'a, O> = nom::IResult<&'a str, O>;

const USERNAME_ALLOWED_CHARS: &str =
    "-.0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";

fn parse(input: &str) -> IResult<Request> {
    let (input, verbose) = opt(verbose)(input)?;
    let verbose = verbose.is_some();

    alt((
        all_consuming(value(Request::new_list(verbose), eof)),
        all_consuming(preceded(
            cond(verbose, space),
            map(
                tuple((opt(is_a(USERNAME_ALLOWED_CHARS)), host_chain)),
                move |(user, forwarding)| Request {
                    verbose,
                    user,
                    forwarding,
                },
            ),
        )),
    ))(input)
}

fn host_chain(input: &str) -> IResult<Option<&str>> {
    opt(recognize(preceded(tag("@"), not_line_ending)))(input)
}

/// Consumes one verbose "/W" flag
fn verbose(input: &str) -> IResult<()> {
    value((), tag("/W"))(input)
}

/// Consumes one or more space " " characters
fn space(input: &str) -> IResult<()> {
    value((), take_while1(|c| c == ' '))(input)
}
