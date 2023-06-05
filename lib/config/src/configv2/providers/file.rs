use super::super::templating::{Template, VarsOnly};
use super::{BufferLimit, ProviderSend};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
pub struct FileProvider {
    path: Template<String, VarsOnly>,
    #[serde(default)]
    repeat: bool,
    #[serde(default)]
    unique: bool,
    auto_return: Option<ProviderSend>,
    #[serde(default)]
    buffer: BufferLimit,
    #[serde(default)]
    format: FileReadFormat,
    #[serde(default)]
    random: bool,
}

/// How the data should be read from the file.
#[derive(Debug, Deserialize, PartialEq, Eq, Default, Clone)]
#[serde(rename_all = "snake_case")]
pub enum FileReadFormat {
    /// Read one line at a time, as either a string or a JSON object.
    /// Json objects that span mulitple lines are not supported in this format.
    #[default]
    Line,
    /// Read the file as a sequence of JSON objects, separated by either whitespace of
    /// self-delineation
    Json,
    /// Read the file as a CSV, with each line being a record, and the first line possibly being
    /// the headers.
    Csv {
        comment: Option<char>,
        #[serde(default = "default_csv_delimiter")]
        delimiter: char,
        #[serde(default = "default_double_quote")]
        double_quote: bool,
        escape: Option<char>,
        #[serde(default)]
        headers: CsvHeaders,
        #[serde(default)]
        terminator: CsvLineTerminator,
        #[serde(default = "default_csv_quote")]
        quote: char,
    },
}

const fn default_csv_delimiter() -> char {
    ','
}

const fn default_double_quote() -> bool {
    true
}

const fn default_csv_quote() -> char {
    '"'
}

/// Define what, if any, headers should be used for each CSV record.
#[derive(Deserialize, Debug, PartialEq, Eq, Clone)]
#[serde(untagged)]
pub enum CsvHeaders {
    /// Specify if the first row should be used as headers, or if no headers should be used.
    Use(bool),
    /// Provide header values directly.
    Provide(Vec<String>),
}

impl Default for CsvHeaders {
    fn default() -> Self {
        Self::Use(false)
    }
}

/// Define what is counted as a terminator that separates multiple CSV records.
#[derive(Deserialize, Debug, PartialEq, Eq, Default, Clone, Copy)]
#[serde(from = "Option<char>")]
pub enum CsvLineTerminator {
    /// Use the provided char
    Provided(char),
    /// Any of the sequences "\n", "\r", or "\r\n" count as a terminator
    #[default]
    JustUseAnyLineEnding,
}

impl From<Option<char>> for CsvLineTerminator {
    fn from(value: Option<char>) -> Self {
        value
            .map(Self::Provided)
            .unwrap_or(Self::JustUseAnyLineEnding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::from_str as from_yaml;

    #[test]
    fn test_csv_headers() {
        let ch = from_yaml::<CsvHeaders>("true").unwrap();
        assert_eq!(ch, CsvHeaders::Use(true));
        let ch = from_yaml::<CsvHeaders>("- hello\n- world").unwrap();
        assert_eq!(
            ch,
            CsvHeaders::Provide(vec!["hello".to_owned(), "world".to_owned()])
        );
    }

    #[test]
    fn test_csv_terminator() {
        let ct = from_yaml::<CsvLineTerminator>("a").unwrap();
        assert_eq!(ct, CsvLineTerminator::Provided('a'));
        let ct = from_yaml::<CsvLineTerminator>("").unwrap();
        assert_eq!(ct, CsvLineTerminator::JustUseAnyLineEnding);
    }

    #[test]
    fn test_file_read_format_basic() {
        let frf = from_yaml::<FileReadFormat>("!line").unwrap();
        assert_eq!(frf, FileReadFormat::Line);
        let frf = from_yaml::<FileReadFormat>("!json").unwrap();
        assert_eq!(frf, FileReadFormat::Json);
    }

    #[test]
    fn test_file_read_format_csv() {
        // defaults
        let frf = from_yaml::<FileReadFormat>("!csv").unwrap();
        let FileReadFormat::Csv {
            comment,
            delimiter,
            double_quote,
            escape,
            headers,
            terminator,
            quote,
        } = frf else { panic!("was not csv") };
        assert_eq!(comment, None);
        assert_eq!(delimiter, ',');
        assert_eq!(double_quote, true);
        assert_eq!(escape, None);
        assert_eq!(headers, CsvHeaders::Use(false));
        assert_eq!(terminator, CsvLineTerminator::JustUseAnyLineEnding);
        assert_eq!(quote, '"');

        // filled
        let frf = from_yaml::<FileReadFormat>(
            r##"
!csv
  comment: "#"
  delimiter: ;
  double_quote: false
  escape: \
  headers: true
  terminator: $
  quote: "'"
        "##,
        )
        .unwrap();
        let FileReadFormat::Csv {
            comment,
            delimiter,
            double_quote,
            escape,
            headers,
            terminator,
            quote,
        } = frf else { panic!("was not csv") };
        assert_eq!(comment, Some('#'));
        assert_eq!(delimiter, ';');
        assert_eq!(double_quote, false);
        assert_eq!(escape, Some('\\'));
        assert_eq!(headers, CsvHeaders::Use(true));
        assert_eq!(terminator, CsvLineTerminator::Provided('$'));
        assert_eq!(quote, '\'');

        // array headers
        let frf = from_yaml(
            r#"
!csv
  headers:
    - foo
    - bar
        "#,
        )
        .unwrap();
        let FileReadFormat::Csv {
            comment,
            delimiter,
            double_quote,
            escape,
            headers,
            terminator,
            quote,
        } = frf else { panic!("was not csv") };
        assert_eq!(comment, None);
        assert_eq!(delimiter, ',');
        assert_eq!(double_quote, true);
        assert_eq!(escape, None);
        assert_eq!(
            headers,
            CsvHeaders::Provide(vec!["foo".to_owned(), "bar".to_owned()])
        );
        assert_eq!(terminator, CsvLineTerminator::JustUseAnyLineEnding);
        assert_eq!(quote, '"');
    }

    #[test]
    fn test_file_provider() {
        static TEST1: &str = "path: !l file.txt";

        let FileProvider {
            path,
            repeat,
            unique,
            auto_return,
            buffer,
            format,
            random,
        } = from_yaml(TEST1).unwrap();
        assert_eq!(
            path,
            Template::Literal {
                value: "file.txt".to_owned()
            }
        );
        assert_eq!(repeat, false);
        assert_eq!(unique, false);
        assert_eq!(auto_return, None);
        assert_eq!(buffer, BufferLimit::Auto);
        assert_eq!(format, FileReadFormat::Line);
        assert_eq!(random, false);

        static TEST2: &str = r"
path: !l file2.txt
repeat: true
unique: true
auto_return: !if_not_full
buffer: 9987
format: !json
random: true
        ";

        let FileProvider {
            path,
            repeat,
            unique,
            auto_return,
            buffer,
            format,
            random,
        } = from_yaml(TEST2).unwrap();
        assert_eq!(
            path,
            Template::Literal {
                value: "file2.txt".to_owned()
            }
        );
        assert_eq!(repeat, true);
        assert_eq!(unique, true);
        assert_eq!(auto_return, Some(ProviderSend::IfNotFull));
        assert_eq!(buffer, BufferLimit::Limit(9987));
        assert_eq!(format, FileReadFormat::Json);
        assert_eq!(random, true);

        static TEST3: &str = r"
path: !l file3.csv
format: !csv
  headers:
    - foo
    - bar";

        let FileProvider {
            path,
            repeat,
            unique,
            auto_return,
            buffer,
            format,
            random,
        } = from_yaml(TEST3).unwrap();
        assert_eq!(
            path,
            Template::Literal {
                value: "file3.csv".to_owned()
            }
        );
        assert_eq!(repeat, false);
        assert_eq!(unique, false);
        assert_eq!(auto_return, None);
        assert_eq!(buffer, BufferLimit::Auto);
        assert_eq!(random, false);
        let FileReadFormat::Csv {
            comment,
            delimiter,
            double_quote,
            escape,
            headers,
            terminator,
            quote,
        } = format else { panic!("was not csv") };
        assert_eq!(comment, None);
        assert_eq!(delimiter, ',');
        assert_eq!(double_quote, true);
        assert_eq!(escape, None);
        assert_eq!(
            headers,
            CsvHeaders::Provide(vec!["foo".to_owned(), "bar".to_owned()])
        );
        assert_eq!(terminator, CsvLineTerminator::JustUseAnyLineEnding);
        assert_eq!(quote, '"');
    }
}
