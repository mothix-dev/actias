extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::{quote, quote_spanned};
use syn::{parse_macro_input, spanned::Spanned, ItemFn};

/// procedural macro to seamlessly declare interrupt handler wrappers
#[proc_macro_attribute]
pub fn interrupt(metadata: TokenStream, input: TokenStream) -> TokenStream {
    let kind = parse_macro_input!(metadata as Ident);
    let input = parse_macro_input!(input as ItemFn);

    // make sure function's signature is ok
    if input.sig.unsafety.is_none() {
        TokenStream::from(quote_spanned! {
            input.sig.span() => compile_error!("interrupt handlers must be unsafe");
        })
    } else if !matches!(input.sig.output, syn::ReturnType::Default) {
        TokenStream::from(quote_spanned! {
            input.sig.output.span() => compile_error!("interrupt handlers cannot return values");
        })
    } else {
        match &*kind.to_string() {
            // x86 interrupt
            "x86" => {
                let name = &input.sig.ident;
                let internal_name = Ident::new(&format!("__internal__{}__", name), Span::call_site());
                let call_asm = format!("call {}", internal_name);
                let inputs = &input.sig.inputs;
                let block = &input.block;

                TokenStream::from(quote! {
                    #[naked]
                    unsafe extern fn #name() -> ! {
                        asm!(
                            "push 0", // push a 0 since no error code was automatically pushed

                            "pusha",

                            "mov ax, ds", // push data segment selector
                            "push eax",

                            "mov ax, 0x10", // switch to kernel's data segment
                            "mov ds, ax",
                            "mov es, ax",
                            "mov fs, ax",
                            "mov gs, ax",

                            "push esp", // pushing a pointer to the registers instead of just interacting with the stored registers on the stack directly prevents many reads or writes from being optimized out

                            #call_asm,

                            "add esp, 4",

                            "pop ebx", // switch back to the old data segment
                            "mov ds, bx",
                            "mov es, bx",
                            "mov fs, bx",
                            "mov gs, bx",

                            "popa",

                            "add esp, 4", // clean up error code

                            "iretd",

                            options(noreturn),
                        );
                    }

                    #[no_mangle]
                    unsafe extern "C" fn #internal_name(#inputs) {
                        #block
                    }
                })
            }
            // x86 exception with error code pushed
            "x86_error_code" => {
                let name = &input.sig.ident;
                let internal_name = Ident::new(&format!("__internal__{}__", name), Span::call_site());
                let call_asm = format!("call {}", internal_name);
                let inputs = &input.sig.inputs;
                let block = &input.block;

                TokenStream::from(quote! {
                    #[naked]
                    unsafe extern fn #name() -> ! {
                        asm!(
                            "pusha",

                            "mov ax, ds", // push data segment selector
                            "push eax",

                            "mov ax, 0x10", // switch to kernel's data segment
                            "mov ds, ax",
                            "mov es, ax",
                            "mov fs, ax",
                            "mov gs, ax",

                            "push esp",

                            #call_asm,

                            "add esp, 4",

                            "pop ebx", // switch back to the old data segment
                            "mov ds, bx",
                            "mov es, bx",
                            "mov fs, bx",
                            "mov gs, bx",

                            "popa",

                            "add esp, 4", // clean up error code

                            "iretd",

                            options(noreturn),
                        );
                    }

                    #[no_mangle]
                    unsafe extern "C" fn #internal_name(#inputs) {
                        #block
                    }
                })
            }
            // unknown interrupt kind
            _ => TokenStream::from(quote_spanned! {
                kind.span() => compile_error!("unsupported interrupt kind");
            }),
        }
    }
}
