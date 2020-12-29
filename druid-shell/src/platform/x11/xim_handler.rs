use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use x11rb::{
    protocol::xproto::{EventMask, KeyPressEvent, KEY_PRESS_EVENT, KEY_RELEASE_EVENT},
    xcb_ffi::XCBConnection,
};
use xim::{
    x11rb::X11rbClient, AttributeName, Client, ClientError, ClientHandler, ForwardEventFlag,
    InputStyle,
};

pub type XimClient = X11rbClient<Rc<XCBConnection>>;

impl ClientHandler<XimClient> for XimHandler {
    fn handle_connect(&mut self, client: &mut XimClient) -> Result<(), ClientError> {
        client.open(b"en_US")
    }

    fn handle_disconnect(&mut self) {}

    fn handle_open(
        &mut self,
        client: &mut XimClient,
        input_method_id: u16,
    ) -> Result<(), ClientError> {
        self.im = input_method_id;

        if let Some(id) = self.create_ic_pend.take() {
            self.spawn_ic(client, id)?;
        }

        Ok(())
    }

    fn handle_close(
        &mut self,
        client: &mut XimClient,
        _input_method_id: u16,
    ) -> Result<(), ClientError> {
        self.im = 0;
        client.disconnect()
    }

    fn handle_query_extension(
        &mut self,
        _client: &mut XimClient,
        _extensions: &[xim::Extension],
    ) -> Result<(), ClientError> {
        Ok(())
    }

    fn handle_get_im_values(
        &mut self,
        _client: &mut XimClient,
        _input_method_id: u16,
        _attributes: xim::AHashMap<xim::AttributeName, Vec<u8>>,
    ) -> Result<(), ClientError> {
        Ok(())
    }

    fn handle_set_ic_values(
        &mut self,
        _client: &mut XimClient,
        _input_method_id: u16,
        _input_context_id: u16,
    ) -> Result<(), ClientError> {
        Ok(())
    }

    fn handle_create_ic(
        &mut self,
        _client: &mut XimClient,
        _input_method_id: u16,
        input_context_id: u16,
    ) -> Result<(), ClientError> {
        let win = self.ic_pend.pop_front().unwrap();
        self.contexts.insert(
            input_context_id,
            XimContext {
                client_win: win,
                forward_event_mask: 0,
            },
        );

        Ok(())
    }

    fn handle_destory_ic(
        &mut self,
        _client: &mut XimClient,
        _input_method_id: u16,
        input_context_id: u16,
    ) -> Result<(), ClientError> {
        let ctx = self.contexts.remove(&input_context_id).unwrap();
        self.ids.remove(&ctx.client_win);
        Ok(())
    }

    fn handle_commit(
        &mut self,
        _client: &mut XimClient,
        _input_method_id: u16,
        _input_context_id: u16,
        text: &str,
    ) -> Result<(), ClientError> {
        // FIXME how to send commit event?
        log::info!("Commit: {}", text);
        Ok(())
    }

    fn handle_forward_event(
        &mut self,
        _client: &mut XimClient,
        _input_method_id: u16,
        _input_context_id: u16,
        _flag: xim::ForwardEventFlag,
        _xev: KeyPressEvent,
    ) -> Result<(), ClientError> {
        // FIXME how to send key event back?
        Ok(())
    }

    fn handle_set_event_mask(
        &mut self,
        _client: &mut XimClient,
        _input_method_id: u16,
        input_context_id: u16,
        forward_event_mask: u32,
        _synchronous_event_mask: u32,
    ) -> Result<(), ClientError> {
        let ctx = self.context(input_context_id)?;
        ctx.forward_event_mask = forward_event_mask;
        Ok(())
    }
}

struct XimContext {
    client_win: u32,
    forward_event_mask: u32,
}

pub struct XimHandler {
    im: u16,
    contexts: HashMap<u16, XimContext>,
    ids: HashMap<u32, u16>,
    ic_pend: VecDeque<u32>,
    create_ic_pend: Option<u32>,
}

impl XimHandler {
    pub fn new() -> Self {
        Self {
            im: 0,
            contexts: HashMap::new(),
            ids: HashMap::new(),
            ic_pend: VecDeque::new(),
            create_ic_pend: None,
        }
    }

    fn context(&mut self, input_context_id: u16) -> Result<&mut XimContext, ClientError> {
        self.contexts
            .get_mut(&input_context_id)
            .ok_or(ClientError::InvalidReply)
    }

    pub fn spawn_ic(&mut self, client: &mut XimClient, id: u32) -> Result<(), ClientError> {
        if self.im == 0 {
            // not yet opened
            assert!(self.create_ic_pend.is_none());
            self.create_ic_pend = Some(id);
        } else {
            let attrs = client
                .build_ic_attributes()
                .push(
                    AttributeName::InputStyle,
                    InputStyle::PREEDITNOTHING | InputStyle::STATUSNOTHING,
                )
                .push(AttributeName::ClientWindow, id)
                .push(AttributeName::FocusWindow, id)
                .build();

            client.create_ic(self.im, attrs)?;
        }

        Ok(())
    }

    pub fn set_preedit_spot(&mut self, client: &mut XimClient, id: u32, spot: xim::Point) -> anyhow::Result<()> {
        let id = if let Some(id) = self.ids.get(&id) {
            *id
        } else {
            return Ok(());
        };

        let attrs = client.build_ic_attributes()
        .nested_list(AttributeName::PreeditAttributes, |b| {
            b.push(AttributeName::SpotLocation, spot);
        })
        .build();

        client.set_ic_values(self.im, id, attrs)?;

        Ok(())
    }

    /// return false when event consumed
    pub fn try_forward_event(
        &mut self,
        client: &mut XimClient,
        id: u32,
        e: &KeyPressEvent,
    ) -> anyhow::Result<bool> {
        let id = if let Some(id) = self.ids.get(&id) {
            *id
        } else {
            return Ok(false);
        };

        if let Some(ctx) = self.contexts.get_mut(&id) {
            if ((ctx.forward_event_mask & (EventMask::KeyPress as u32) != 0)
                && e.response_type == KEY_PRESS_EVENT)
                || ((ctx.forward_event_mask & (EventMask::KeyRelease as u32) != 0)
                    && e.response_type == KEY_RELEASE_EVENT)
            {
                client.forward_event(self.im, id, ForwardEventFlag::empty(), e)?;
                return Ok(true);
            }
        }

        Ok(false)
    }
}
