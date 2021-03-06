/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// https://www.khronos.org/registry/webgl/specs/latest/1.0/webgl.idl
use canvas_traits::webgl::{WebGLCommand, WebGLFramebufferBindingRequest, WebGLFramebufferId};
use canvas_traits::webgl::{WebGLResult, WebGLError, webgl_channel};
use dom::bindings::cell::DomRefCell;
use dom::bindings::codegen::Bindings::WebGLFramebufferBinding;
use dom::bindings::codegen::Bindings::WebGLRenderingContextBinding::WebGLRenderingContextConstants as constants;
use dom::bindings::inheritance::Castable;
use dom::bindings::reflector::{DomObject, reflect_dom_object};
use dom::bindings::root::{Dom, DomRoot};
use dom::webglobject::WebGLObject;
use dom::webglrenderbuffer::WebGLRenderbuffer;
use dom::webglrenderingcontext::WebGLRenderingContext;
use dom::webgltexture::WebGLTexture;
use dom_struct::dom_struct;
use std::cell::Cell;

#[must_root]
#[derive(Clone, JSTraceable, MallocSizeOf)]
enum WebGLFramebufferAttachment {
    Renderbuffer(Dom<WebGLRenderbuffer>),
    Texture { texture: Dom<WebGLTexture>, level: i32 },
}

#[derive(Clone, JSTraceable, MallocSizeOf)]
pub enum WebGLFramebufferAttachmentRoot {
    Renderbuffer(DomRoot<WebGLRenderbuffer>),
    Texture(DomRoot<WebGLTexture>),
}

#[dom_struct]
pub struct WebGLFramebuffer {
    webgl_object: WebGLObject,
    id: WebGLFramebufferId,
    /// target can only be gl::FRAMEBUFFER at the moment
    target: Cell<Option<u32>>,
    is_deleted: Cell<bool>,
    size: Cell<Option<(i32, i32)>>,
    status: Cell<u32>,
    // The attachment points for textures and renderbuffers on this
    // FBO.
    color: DomRefCell<Option<WebGLFramebufferAttachment>>,
    depth: DomRefCell<Option<WebGLFramebufferAttachment>>,
    stencil: DomRefCell<Option<WebGLFramebufferAttachment>>,
    depthstencil: DomRefCell<Option<WebGLFramebufferAttachment>>,
}

impl WebGLFramebuffer {
    fn new_inherited(context: &WebGLRenderingContext, id: WebGLFramebufferId) -> Self {
        Self {
            webgl_object: WebGLObject::new_inherited(context),
            id: id,
            target: Cell::new(None),
            is_deleted: Cell::new(false),
            size: Cell::new(None),
            status: Cell::new(constants::FRAMEBUFFER_UNSUPPORTED),
            color: DomRefCell::new(None),
            depth: DomRefCell::new(None),
            stencil: DomRefCell::new(None),
            depthstencil: DomRefCell::new(None),
        }
    }

    pub fn maybe_new(context: &WebGLRenderingContext) -> Option<DomRoot<Self>> {
        let (sender, receiver) = webgl_channel().unwrap();
        context.send_command(WebGLCommand::CreateFramebuffer(sender));
        receiver.recv().unwrap().map(|id| WebGLFramebuffer::new(context, id))
    }

    pub fn new(
        context: &WebGLRenderingContext,
        id: WebGLFramebufferId,
    ) -> DomRoot<Self> {
        reflect_dom_object(
            Box::new(WebGLFramebuffer::new_inherited(context, id)),
            &*context.global(),
            WebGLFramebufferBinding::Wrap,
        )
    }
}


impl WebGLFramebuffer {
    pub fn id(&self) -> WebGLFramebufferId {
        self.id
    }

    pub fn bind(&self, target: u32) {
        // Update the framebuffer status on binding.  It may have
        // changed if its attachments were resized or deleted while
        // we've been unbound.
        self.update_status();

        self.target.set(Some(target));
        self.upcast::<WebGLObject>().context().send_command(
            WebGLCommand::BindFramebuffer(target, WebGLFramebufferBindingRequest::Explicit(self.id)),
        );
    }

    pub fn delete(&self) {
        if !self.is_deleted.get() {
            self.is_deleted.set(true);
            self.upcast::<WebGLObject>()
                .context()
                .send_command(WebGLCommand::DeleteFramebuffer(self.id));
        }
    }

    pub fn is_deleted(&self) -> bool {
        self.is_deleted.get()
    }

    pub fn size(&self) -> Option<(i32, i32)> {
        self.size.get()
    }

    fn update_status(&self) {
        let c = self.color.borrow();
        let z = self.depth.borrow();
        let s = self.stencil.borrow();
        let zs = self.depthstencil.borrow();
        let has_c = c.is_some();
        let has_z = z.is_some();
        let has_s = s.is_some();
        let has_zs = zs.is_some();
        let attachments = [&*c, &*z, &*s, &*zs];

        // From the WebGL spec, 6.6 ("Framebuffer Object Attachments"):
        //
        //    "In the WebGL API, it is an error to concurrently attach
        //     renderbuffers to the following combinations of
        //     attachment points:
        //
        //     DEPTH_ATTACHMENT + DEPTH_STENCIL_ATTACHMENT
        //     STENCIL_ATTACHMENT + DEPTH_STENCIL_ATTACHMENT
        //     DEPTH_ATTACHMENT + STENCIL_ATTACHMENT
        //
        //     If any of the constraints above are violated, then:
        //
        //     checkFramebufferStatus must return FRAMEBUFFER_UNSUPPORTED."
        if (has_zs && (has_z || has_s)) ||
            (has_z && has_s) {
            self.status.set(constants::FRAMEBUFFER_UNSUPPORTED);
            return;
        }

        let mut fb_size = None;
        for attachment in &attachments {
            // Get the size of this attachment.
            let size = match **attachment {
                Some(WebGLFramebufferAttachment::Renderbuffer(ref att_rb)) => {
                    att_rb.size()
                }
                Some(WebGLFramebufferAttachment::Texture { texture: ref att_tex, level } ) => {
                    let info = att_tex.image_info_at_face(0, level as u32);
                    Some((info.width() as i32, info.height() as i32))
                }
                None => None,
            };

            // Make sure that, if we've found any other attachment,
            // that the size matches.
            if size.is_some() {
                if fb_size.is_some() && size != fb_size {
                    self.status.set(constants::FRAMEBUFFER_INCOMPLETE_DIMENSIONS);
                    return;
                } else {
                    fb_size = size;
                }
            }
        }
        self.size.set(fb_size);

        if has_c || has_z || has_zs || has_s {
            if self.size.get().map_or(false, |(w, h)| w != 0 && h != 0) {
                self.status.set(constants::FRAMEBUFFER_COMPLETE);
            } else {
                self.status.set(constants::FRAMEBUFFER_INCOMPLETE_ATTACHMENT);
            }
        } else {
            self.status.set(constants::FRAMEBUFFER_UNSUPPORTED);
        }
    }

    pub fn check_status(&self) -> u32 {
        return self.status.get();
    }

    pub fn renderbuffer(&self, attachment: u32, rb: Option<&WebGLRenderbuffer>) -> WebGLResult<()> {
        let binding = match attachment {
            constants::COLOR_ATTACHMENT0 => &self.color,
            constants::DEPTH_ATTACHMENT => &self.depth,
            constants::STENCIL_ATTACHMENT => &self.stencil,
            constants::DEPTH_STENCIL_ATTACHMENT => &self.depthstencil,
            _ => return Err(WebGLError::InvalidEnum),
        };

        let rb_id = match rb {
            Some(rb) => {
                *binding.borrow_mut() = Some(WebGLFramebufferAttachment::Renderbuffer(Dom::from_ref(rb)));
                Some(rb.id())
            }

            _ => {
                *binding.borrow_mut() = None;
                None
            }
        };

        self.upcast::<WebGLObject>().context().send_command(
            WebGLCommand::FramebufferRenderbuffer(
                constants::FRAMEBUFFER,
                attachment,
                constants::RENDERBUFFER,
                rb_id,
            ),
        );

        self.update_status();
        Ok(())
    }

    pub fn attachment(&self, attachment: u32) -> Option<WebGLFramebufferAttachmentRoot> {
        let binding = match attachment {
            constants::COLOR_ATTACHMENT0 => &self.color,
            constants::DEPTH_ATTACHMENT => &self.depth,
            constants::STENCIL_ATTACHMENT => &self.stencil,
            constants::DEPTH_STENCIL_ATTACHMENT => &self.depthstencil,
            _ => return None,
        };

        binding.borrow().as_ref().map(|bin| {
            match bin {
                &WebGLFramebufferAttachment::Renderbuffer(ref rb) =>
                    WebGLFramebufferAttachmentRoot::Renderbuffer(DomRoot::from_ref(&rb)),
                &WebGLFramebufferAttachment::Texture { ref texture, .. } =>
                    WebGLFramebufferAttachmentRoot::Texture(DomRoot::from_ref(&texture)),
            }
        })
    }

    pub fn texture2d(&self, attachment: u32, textarget: u32, texture: Option<&WebGLTexture>,
                     level: i32) -> WebGLResult<()> {
        let binding = match attachment {
            constants::COLOR_ATTACHMENT0 => &self.color,
            constants::DEPTH_ATTACHMENT => &self.depth,
            constants::STENCIL_ATTACHMENT => &self.stencil,
            constants::DEPTH_STENCIL_ATTACHMENT => &self.depthstencil,
            _ => return Err(WebGLError::InvalidEnum),
        };

        let tex_id = match texture {
            // Note, from the GLES 2.0.25 spec, page 113:
            //      "If texture is zero, then textarget and level are ignored."
            Some(texture) => {
                // From the GLES 2.0.25 spec, page 113:
                //
                //     "level specifies the mipmap level of the texture image
                //      to be attached to the framebuffer and must be
                //      0. Otherwise, INVALID_VALUE is generated."
                if level != 0 {
                    return Err(WebGLError::InvalidValue);
                }

                //     "If texture is not zero, then texture must either
                //      name an existing texture object with an target of
                //      textarget, or texture must name an existing cube
                //      map texture and textarget must be one of:
                //      TEXTURE_CUBE_MAP_POSITIVE_X,
                //      TEXTURE_CUBE_MAP_POSITIVE_Y,
                //      TEXTURE_CUBE_MAP_POSITIVE_Z,
                //      TEXTURE_CUBE_MAP_NEGATIVE_X,
                //      TEXTURE_CUBE_MAP_NEGATIVE_Y, or
                //      TEXTURE_CUBE_MAP_NEGATIVE_Z. Otherwise,
                //      INVALID_OPERATION is generated."
                let is_cube = match textarget {
                    constants::TEXTURE_2D => false,

                    constants::TEXTURE_CUBE_MAP_POSITIVE_X => true,
                    constants::TEXTURE_CUBE_MAP_POSITIVE_Y => true,
                    constants::TEXTURE_CUBE_MAP_POSITIVE_Z => true,
                    constants::TEXTURE_CUBE_MAP_NEGATIVE_X => true,
                    constants::TEXTURE_CUBE_MAP_NEGATIVE_Y => true,
                    constants::TEXTURE_CUBE_MAP_NEGATIVE_Z => true,

                    _ => return Err(WebGLError::InvalidEnum),
                };

                match texture.target() {
                    Some(constants::TEXTURE_CUBE_MAP) if is_cube => {}
                    Some(_) if !is_cube => {}
                    _ => return Err(WebGLError::InvalidOperation),
                }

                *binding.borrow_mut() = Some(WebGLFramebufferAttachment::Texture {
                    texture: Dom::from_ref(texture),
                    level: level }
                );

                Some(texture.id())
            }

            _ => {
                *binding.borrow_mut() = None;
                None
            }
        };

        self.upcast::<WebGLObject>().context().send_command(
            WebGLCommand::FramebufferTexture2D(
                constants::FRAMEBUFFER,
                attachment,
                textarget,
                tex_id,
                level,
            ),
        );

        self.update_status();
        Ok(())
    }

    fn with_matching_renderbuffers<F>(&self, rb: &WebGLRenderbuffer, mut closure: F)
        where F: FnMut(&DomRefCell<Option<WebGLFramebufferAttachment>>)
    {
        let attachments = [&self.color,
                           &self.depth,
                           &self.stencil,
                           &self.depthstencil];

        for attachment in &attachments {
            let matched = {
                match *attachment.borrow() {
                    Some(WebGLFramebufferAttachment::Renderbuffer(ref att_rb))
                        if rb.id() == att_rb.id() => true,
                    _ => false,
                }
            };

            if matched {
                closure(attachment);
            }
        }
    }

    fn with_matching_textures<F>(&self, texture: &WebGLTexture, mut closure: F)
        where F: FnMut(&DomRefCell<Option<WebGLFramebufferAttachment>>)
    {
        let attachments = [&self.color,
                           &self.depth,
                           &self.stencil,
                           &self.depthstencil];

        for attachment in &attachments {
            let matched = {
                match *attachment.borrow() {
                    Some(WebGLFramebufferAttachment::Texture { texture: ref att_texture, .. })
                        if texture.id() == att_texture.id() => true,
                    _ => false,
                }
            };

            if matched {
                closure(attachment);
            }
        }
    }

    pub fn detach_renderbuffer(&self, rb: &WebGLRenderbuffer) {
        self.with_matching_renderbuffers(rb, |att| {
            *att.borrow_mut() = None;
            self.update_status();
        });
    }

    pub fn detach_texture(&self, texture: &WebGLTexture) {
        self.with_matching_textures(texture, |att| {
            *att.borrow_mut() = None;
            self.update_status();
        });
    }

    pub fn invalidate_renderbuffer(&self, rb: &WebGLRenderbuffer) {
        self.with_matching_renderbuffers(rb, |_att| {
            self.update_status();
        });
    }

    pub fn invalidate_texture(&self, texture: &WebGLTexture) {
        self.with_matching_textures(texture, |_att| {
            self.update_status();
        });
    }

    pub fn target(&self) -> Option<u32> {
        self.target.get()
    }
}

impl Drop for WebGLFramebuffer {
    fn drop(&mut self) {
        self.delete();
    }
}
