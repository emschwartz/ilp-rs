use byteorder::ReadBytesExt;
use bytes::Bytes;
use chrono::{Duration, Utc};
use errors::ParseError;
use futures::Future;
use ilp::{IlpFulfill, IlpPacket, IlpPrepare};
use oer::ReadOerExt;
use plugin::Plugin;
use std::io::Cursor;

static ILDCP_DESTINATION: &'static str = "peer.config";
lazy_static! {
  static ref PEER_PROTOCOL_EXPIRY_DURATION: Duration = Duration::minutes(1);
  static ref PEER_PROTOCOL_FULFILLMENT: Bytes = Bytes::from(vec![0; 32]);
  static ref PEER_PROTOCOL_CONDITION: Bytes = Bytes::from(vec![
    102, 104, 122, 173, 248, 98, 189, 119, 108, 143, 193, 139, 142, 159, 142, 32, 8, 151, 20, 133,
    110, 226, 51, 179, 144, 42, 89, 29, 13, 95, 41, 37
  ]);
}

#[derive(Debug)]
pub struct IldcpRequest {}

impl IldcpRequest {
  pub fn new() -> Self {
    IldcpRequest {}
  }

  pub fn to_prepare(&self) -> IlpPrepare {
    IlpPrepare::new(
      ILDCP_DESTINATION,
      0,
      &PEER_PROTOCOL_CONDITION[..],
      Utc::now() + *PEER_PROTOCOL_EXPIRY_DURATION,
      Bytes::new(),
    )
  }
}

#[derive(Debug)]
pub struct IldcpResponse {
  pub client_address: String,
  pub asset_scale: u8,
  pub asset_code: String,
}

impl IldcpResponse {
  pub fn from_fulfill(fulfill: IlpFulfill) -> Result<Self, ParseError> {
    let mut reader = Cursor::new(&fulfill.data[..]);
    let client_address = String::from_utf8(reader.read_var_octet_string()?)?;
    let asset_scale = reader.read_u8()?;
    let asset_code = String::from_utf8(reader.read_var_octet_string()?)?;
    Ok(IldcpResponse {
      client_address,
      asset_scale,
      asset_code,
    })
  }
}

// On error only returns the plugin if it can continue to be used
pub fn get_config(
  plugin: impl Plugin,
) -> impl Future<Item = (IldcpResponse, impl Plugin), Error = ((), Option<impl Plugin>)> {
  let prepare = IldcpRequest::new().to_prepare();
  // TODO make sure this doesn't conflict with other packets
  let original_request_id = 0;
  plugin
    .send((original_request_id, IlpPacket::Prepare(prepare)))
    .map_err(move |err| {
      error!("Error sending ILDCP request {:?}", err);
      // TODO do we need to return the plugin here?
      ((), None)
    }).and_then(|plugin| {
      plugin
        .into_future()
        .and_then(|(next, plugin)| {
          if let Some((request_id, IlpPacket::Fulfill(fulfill))) = next {
            if let Ok(response) = IldcpResponse::from_fulfill(fulfill) {
              debug!("Got ILDCP response: {:?}", response);
              Ok((response, plugin))
            } else {
              error!("Unable to parse ILDCP response from fulfill");
              Err(((), plugin))
            }
          } else {
            error!(
              "Expected Fulfill packet in response to ILDCP request, got: {:?}",
              next
            );
            Err(((), plugin))
          }
        }).map_err(|(err, plugin)| (err, Some(plugin)))
    })
}
