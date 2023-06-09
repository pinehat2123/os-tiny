#![no_std]
#![feature(alloc)]
#![feature(rustc_private)]
#![feature(transpose_result)]

#[macro_use] extern crate alloc;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;
extern crate spin;
extern crate irq_safety;
extern crate xmas_elf;
extern crate memory;
extern crate kernel_config;
extern crate goblin;
extern crate util;
extern crate rustc_demangle;
extern crate owning_ref;
extern crate cow_arc;
extern crate hashmap_core;
extern crate qp_trie;


use core::ops::DerefMut;
use alloc::vec::Vec;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::{String, ToString};
use alloc::sync::{Arc, Weak};
use spin::Mutex;

use xmas_elf::ElfFile;
use xmas_elf::sections::{SectionData, ShType};
use xmas_elf::sections::{SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR};
use goblin::elf::reloc::*;

use util::round_up_power_of_two;
use memory::{FRAME_ALLOCATOR, get_module_starting_with, MemoryManagementInfo, ModuleArea, Frame, PageTable, VirtualAddress, MappedPages, EntryFlags, allocate_pages_by_bytes};
use metadata::{StrongCrateRef, WeakSectionRef};
use cow_arc::CowArc;
use hashmap_core::HashMap;
use rustc_demangle::demangle;
use qp_trie::{Trie, Entry, wrapper::BString};


pub mod elf_executable {
    use alloc::vec::Vec;
    use memory::{MappedPages, VirtualMemoryArea, VirtualAddress};

    use xmas_elf::ElfFile;


    /// A program segment in an ELF file that has been loaded and is directly executable. 
    pub struct ElfProgramSegment {
        /// the VirtualMemoryAddress that will represent the virtual mapping of this Program segment.
        /// Provides starting virtual address, size in memory, mapping flags, and a text description.
        pub vma: VirtualMemoryArea,
        /// the offset of this segment into the file.
        /// This plus the physical address of the Elf file is the physical address of this Program segment.
        pub offset: usize,
    }


    /// Parses an elf executable file as a slice of bytes starting at the given `MappedPages` mapping.
    /// Consumes the given `MappedPages`, which automatically unmaps it at the end of this function. 
    pub fn parse_elf_executable(mapped_pages: MappedPages, size_in_bytes: usize) -> Result<(Vec<ElfProgramSegment>, VirtualAddress), &'static str> {
        debug!("Parsing Elf executable: mapped_pages {:?}, size_in_bytes {:#x}({})", mapped_pages, size_in_bytes, size_in_bytes);

        let byte_slice: &[u8] = try!(mapped_pages.as_slice(0, size_in_bytes));
        let elf_file = try!(ElfFile::new(byte_slice));
        // debug!("Elf File: {:?}", elf_file);

        // check that elf_file is an executable type 
        {
            use xmas_elf::header::Type;
            let typ = elf_file.header.pt2.type_().as_type();
            if typ != Type::Executable {
                error!("parse_elf_executable(): ELF file has wrong type {:?}, must be an Executable Elf File!", typ);
                return Err("not a relocatable elf file");
            }
        } 

        let mut prog_sects: Vec<ElfProgramSegment> = Vec::new();
        for prog in elf_file.program_iter() {
            // debug!("   prog: {}", prog);
            use xmas_elf::program::Type;
            let typ = prog.get_type();
            if typ != Ok(Type::Load) {
                warn!("Program type in ELF file wasn't LOAD, {}", prog);
                return Err("Program type in ELF file wasn't LOAD");
            }
            let flags = EntryFlags::from_elf_program_flags(prog.flags());
            use memory::*;
            if !flags.contains(EntryFlags::PRESENT) {
                warn!("Program flags in ELF file wasn't Read, so EntryFlags wasn't PRESENT!! {}", prog);
                return Err("Program flags in ELF file wasn't Read, so EntryFlags wasn't PRESENT!");
            }
            // TODO: how to get name of program section?
            // could infer it based on perms, like .text or .data
            prog_sects.push(ElfProgramSegment {
                vma: VirtualMemoryArea::new(prog.virtual_addr() as VirtualAddress, prog.mem_size() as usize, flags, "test_name"),
                offset: prog.offset() as usize,
            });
        }

        let entry_point = elf_file.header.pt2.entry_point() as VirtualAddress;

        Ok((prog_sects, entry_point))
    }
}
pub mod parse_nano_core {
    //! Routines for parsing the `nano_core`, the fully-linked, already loaded code that is currently executing.
    //! As such, it performs no loading, but rather just creates metadata that represents
    //! the existing kernel code that was loaded by the bootloader, and adds those functions to the system map.

    use core::ops::DerefMut;
    use alloc::string::{String, ToString};
    use alloc::collections::{BTreeMap, BTreeSet};
    use alloc::sync::Arc;
    use spin::Mutex;
    use cow_arc::CowArc;

    use xmas_elf;
    use xmas_elf::ElfFile;
    use xmas_elf::sections::ShType;
    use xmas_elf::sections::{SHF_WRITE, SHF_ALLOC, SHF_EXECINSTR};
    use rustc_demangle::demangle;

    use memory::{FRAME_ALLOCATOR, ModuleArea, get_module, MemoryManagementInfo, Frame, PageTable, VirtualAddress, MappedPages, EntryFlags, allocate_pages_by_bytes};
    use metadata::{CrateType, LoadedCrate, StrongCrateRef, LoadedSection, StrongSectionRef, SectionType};
    use qp_trie::Trie;


    /// Decides which parsing technique to use, either the symbol file or the actual binary file.
    /// `parse_nano_core_binary()` is VERY SLOW in debug mode for large binaries, so we use the more efficient symbol file parser instead.
    /// Note that this must match the setup of the kernel/Makefile as well as the cfg/grub.cfg entry
    const PARSE_NANO_CORE_SYMBOL_FILE: bool = true;



    /// Just like Rust's `try!()` macro, but packages up the given error message in a tuple
    /// with the array of 3 MappedPages that must also be returned. 
    macro_rules! try_mp {
        ($expr:expr, $tp:expr, $rp:expr, $dp:expr) => (match $expr {
            Ok(val) => val,
            Err(err_msg) => return Err((err_msg, [$tp, $rp, $dp])),
        });
    }


    /// Just like Rust's `try!()` macro, but sets the given error message
    /// and then breaks out of the loop;
    /// with the array of 3 MappedPages that must also be returned. 
    macro_rules! try_break {
        ($expr:expr, $result:ident) => (match $expr {
            Ok(val) => val,
            Err(err) => {
                $result = Err(err);
                break;
            }
        });
    }


    /// Parses the nano_core module that represents the already loaded (and currently running) nano_core code.
    /// Basically, just searches for global (public) symbols, which are added to the system map and the crate metadata.
    /// 
    /// If an error occurs, the returned `Result::Err` contains the passed-in `text_pages`, `rodata_pages`, and `data_pages`
    /// because those cannot be dropped, as they hold the currently-running code, and dropping them would cause endless exceptions.
    pub fn parse_nano_core(
        kernel_mmi:   &mut MemoryManagementInfo, 
        text_pages:   MappedPages, 
        rodata_pages: MappedPages, 
        data_pages:   MappedPages, 
        verbose_log: bool
    ) -> Result<usize, (&'static str, [Arc<Mutex<MappedPages>>; 3])> {

        let text_pages   = Arc::new(Mutex::new(text_pages));
        let rodata_pages = Arc::new(Mutex::new(rodata_pages));
        let data_pages   = Arc::new(Mutex::new(data_pages));

        let crate_name = String::from("nano_core");
        debug!("parse_nano_core: trying to load and parse the nano_core file");
        let module = try_mp!(
            get_module(&format!("{}nano_core", CrateType::Kernel.prefix())).ok_or("Couldn't find nano_core module"), 
            text_pages, rodata_pages, data_pages
        );
        use kernel_config::memory::address_is_page_aligned;
        if !address_is_page_aligned(module.start_address()) {
            error!("module {} is not page aligned!", module.name());
            return Err(("nano_core module was not page aligned", [text_pages, rodata_pages, data_pages]));
        } 

        // below, we map the nano_core module file just so we can parse it. We don't need to actually load it since we're already running it.
        if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
            let (size, flags) = if PARSE_NANO_CORE_SYMBOL_FILE {
                (
                    // + 1 to add space for appending a null character to the end of the symbol file string
                    module.size() + 1, 
                    // WRITABLE because we need to write that null character
                    EntryFlags::PRESENT | EntryFlags::WRITABLE
                )
            }
            else {
                (module.size(), EntryFlags::PRESENT)
            };

            let temp_module_mapping = {
                let new_pages = try_mp!(allocate_pages_by_bytes(size).ok_or("couldn't allocate pages for nano_core module"), text_pages, rodata_pages, data_pages);
                let mut frame_allocator = try_mp!(FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME_ALLOCATOR"), text_pages, rodata_pages, data_pages).lock();
                try_mp!(active_table.map_allocated_pages_to(
                    new_pages, Frame::range_inclusive_addr(module.start_address(), size), 
                    flags, frame_allocator.deref_mut()
                ), text_pages, rodata_pages, data_pages)
            };

            let new_crate_ref = if PARSE_NANO_CORE_SYMBOL_FILE {
                parse_nano_core_symbol_file(temp_module_mapping, module, text_pages, rodata_pages, data_pages, size)?
            } else {
                parse_nano_core_binary(temp_module_mapping, module, text_pages, rodata_pages, data_pages, size)?
            };

            let default_namespace = super::get_default_namespace();
            trace!("parse_nano_core(): adding symbols to namespace...");
            let new_syms = default_namespace.add_symbols(new_crate_ref.lock_as_ref().sections.values(), verbose_log);
            trace!("parse_nano_core(): finished adding symbols.");
            default_namespace.crate_tree.lock().insert(crate_name.into(), new_crate_ref);
            info!("parsed nano_core crate, {} new symbols.", new_syms);
            Ok(new_syms)

            // temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
        }
        else {
            error!("parse_nano_core(): error getting kernel's active page table to map module.");
            Err(("couldn't get kernel's active page table", [text_pages, rodata_pages, data_pages]))
        }
    }



    /// Parses the nano_core symbol file that represents the already loaded (and currently running) nano_core code.
    /// Basically, just searches for global (public) symbols, which are added to the system map and the crate metadata.
    /// 
    /// Drops the given `mapped_pages` that hold the nano_core module file itself.
    fn parse_nano_core_symbol_file(
        mut mapped_pages: MappedPages,
        nano_core_object_file: &'static ModuleArea,
        text_pages:   Arc<Mutex<MappedPages>>,
        rodata_pages: Arc<Mutex<MappedPages>>,
        data_pages:   Arc<Mutex<MappedPages>>,
        size: usize
    ) -> Result<StrongCrateRef, (&'static str, [Arc<Mutex<MappedPages>>; 3])> {
        let crate_name = String::from("nano_core");
        debug!("Parsing nano_core symbols: size {:#x}({}), mapped_pages: {:?}, text_pages: {:?}, rodata_pages: {:?}, data_pages: {:?}", 
            size, size, mapped_pages, text_pages, rodata_pages, data_pages);

        // because the nano_core doesn't have one section per function/data/rodata, we fake it here with an arbitrary section counter
        let mut section_counter = 0;
        let mut sections: BTreeMap<usize, StrongSectionRef> = BTreeMap::new();

        // ensure that there's a null byte at the end
        {
            let null_byte: &mut u8 = try_mp!(mapped_pages.as_type_mut(size - 1), text_pages, rodata_pages, data_pages);
            *null_byte = 0u8;
        }

        let new_crate = CowArc::new(LoadedCrate {
            crate_name:              crate_name,
            object_file:             nano_core_object_file,
            sections:                BTreeMap::new(),
            text_pages:              Some(text_pages.clone()),
            rodata_pages:            Some(rodata_pages.clone()),
            data_pages:              Some(data_pages.clone()),
            global_symbols:          BTreeSet::new(),
            bss_sections:            Trie::new(),
            reexported_symbols:      BTreeSet::new(),
        });
        let new_crate_weak_ref = CowArc::downgrade(&new_crate);

        // scoped to drop the borrow on mapped_pages through `bytes`
        {
            use util::c_str::CStr;
            let bytes = try_mp!(mapped_pages.as_slice_mut(0, size), text_pages, rodata_pages, data_pages);
            let symbol_cstr = try_mp!(CStr::from_bytes_with_nul(bytes).map_err(|e| {
                error!("parse_nano_core_symbol_file(): error casting memory to CStr: {:?}", e);
                "FromBytesWithNulError occurred when casting nano_core symbol memory to CStr"
            }), text_pages, rodata_pages, data_pages);
            let symbol_str = try_mp!(symbol_cstr.to_str().map_err(|e| {
                error!("parse_nano_core_symbol_file(): error with CStr::to_str(): {:?}", e);
                "Utf8Error occurred when parsing nano_core symbols CStr"
            }), text_pages, rodata_pages, data_pages);

            // debug!("========================= NANO_CORE SYMBOL STRING ========================\n{}", symbol_str);


            let mut text_shndx:   Option<usize> = None;
            let mut data_shndx:   Option<usize> = None;
            let mut rodata_shndx: Option<usize> = None;
            let mut bss_shndx:    Option<usize> = None;

            
            // a closure that parses a section index out of a string like "[7]"
            let parse_section_ndx = |str_ref: &str| {
                let open  = str_ref.find("[");
                let close = str_ref.find("]");
                open.and_then(|start| close.and_then(|end| str_ref.get((start + 1) .. end)))
                    .and_then(|t| t.trim().parse::<usize>().ok())
            };

            // first, find the section indices that we care about: .text, .data, .rodata, and .bss
            let file_iterator = symbol_str.lines().enumerate();
            for (_line_num, line) in file_iterator.clone() {

                // skip empty lines
                let line = line.trim();
                if line.is_empty() { continue; }

                // debug!("Looking at line: {:?}", line);

                if line.contains(".text") && line.contains("PROGBITS") {
                    text_shndx = parse_section_ndx(line);
                }
                else if line.contains(".data") && line.contains("PROGBITS") {
                    data_shndx = parse_section_ndx(line);
                }
                else if line.contains(".rodata") && line.contains("PROGBITS") {
                    rodata_shndx = parse_section_ndx(line);
                }
                else if line.contains(".bss") && line.contains("NOBITS") {
                    bss_shndx = parse_section_ndx(line);
                }

                // once we've found the 4 sections we care about, we're done
                if text_shndx.is_some() && rodata_shndx.is_some() && data_shndx.is_some() && bss_shndx.is_some() {
                    break;
                }
            }

            let text_shndx   = try_mp!(text_shndx  .ok_or("parse_nano_core_symbol_file(): couldn't find .text section index"),   text_pages, rodata_pages, data_pages);
            let rodata_shndx = try_mp!(rodata_shndx.ok_or("parse_nano_core_symbol_file(): couldn't find .rodata section index"), text_pages, rodata_pages, data_pages);
            let data_shndx   = try_mp!(data_shndx  .ok_or("parse_nano_core_symbol_file(): couldn't find .data section index"),   text_pages, rodata_pages, data_pages);
            let bss_shndx    = try_mp!(bss_shndx   .ok_or("parse_nano_core_symbol_file(): couldn't find .bss section index"),    text_pages, rodata_pages, data_pages);


            // second, skip ahead to the start of the symbol table 
            let mut file_iterator = file_iterator.skip_while( | (_line_num, line) |  {
                !line.starts_with("Symbol table")
            });
            // skip the symbol table start line, e.g., "Symbol table '.symtab' contains N entries:"
            if let Some((_num, _line)) = file_iterator.next() {
                // trace!("SKIPPING LINE {}: {}", _num + 1, _line);
            }
            // skip one more line, the line with the column headers, e.g., "Num:     Value     Size Type   Bind   Vis ..."
            if let Some((_num, _line)) = file_iterator.next() {
                // trace!("SKIPPING LINE {}: {}", _num + 1, _line);
            }

            // a placeholder for any error that might occur during the loop below
            let mut loop_result: Result<(), &'static str> = Ok(());

            {
                let text_pages_locked = text_pages.lock();
                let rodata_pages_locked = rodata_pages.lock();
                let data_pages_locked = data_pages.lock();

                // third, parse each symbol table entry, which should all have "GLOBAL" bindings
                for (_line_num, line) in file_iterator {
                    if line.is_empty() { continue; }
                    
                    // we need the following items from a symbol table entry:
                    // * Value (address),      column 1
                    // * Size,                 column 2
                    // * Ndx,                  column 6
                    // * DemangledName#hash    column 7 to end

                    // Can't use split_whitespace() here, because we need to splitn and then get the remainder of the line
                    // after we've split the first 7 columns by whitespace. So we write a custom closure to group multiple whitespaces together.\
                    // We use "splitn(8, ..)" because it stops at the 8th column (column index 7) and gets the rest of the line in a single iteration.
                    let mut prev_whitespace = true; // by default, we start assuming that the previous element was whitespace.
                    let mut parts = line.splitn(8, |c: char| {
                        if c.is_whitespace() {
                            if prev_whitespace {
                                false
                            } else {
                                prev_whitespace = true;
                                true
                            }
                        } else {
                            prev_whitespace = false;
                            false
                        }
                    }).map(str::trim);

                    let _num      = try_break!(parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 0 'Num'"),   loop_result);
                    let sec_vaddr = try_break!(parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 1 'Value'"), loop_result);
                    let sec_size  = try_break!(parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 2 'Size'"),  loop_result);
                    let _typ      = try_break!(parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 3 'Type'"),  loop_result);
                    let _bind     = try_break!(parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 4 'Bind'"),  loop_result);
                    let _vis      = try_break!(parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 5 'Vis'"),   loop_result);
                    let sec_ndx   = try_break!(parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 6 'Ndx'"),   loop_result);
                    let name      = try_break!(parts.next().ok_or("parse_nano_core_symbol_file(): couldn't get column 7 'Name'"),  loop_result);
                    
                    let sec_vaddr = try_break!(usize::from_str_radix(sec_vaddr, 16)
                        .map_err(|e| {
                            error!("parse_nano_core_symbol_file(): error parsing virtual address Value at line {}: {:?}\n    line: {}", _line_num + 1, e, line);
                            "parse_nano_core_symbol_file(): couldn't parse virtual address (value column)"
                        }),
                        loop_result
                    );
                    let sec_size = try_break!(usize::from_str_radix(sec_size, 10)
                        .or_else(|e| {
                            sec_size.get(2 ..).ok_or(e).and_then(|sec_size_hex| 
                                usize::from_str_radix(sec_size_hex, 16)
                            )
                        })
                        .map_err(|e| {
                            error!("parse_nano_core_symbol_file(): error parsing size at line {}: {:?}\n    line: {}", _line_num + 1, e, line);
                            "parse_nano_core_symbol_file(): couldn't parse size column"
                        }),
                        loop_result
                    ); 

                    // while vaddr and size are required, ndx could be valid or not. 
                    let sec_ndx = match usize::from_str_radix(sec_ndx, 10) {
                        // If ndx is a valid number, proceed on. 
                        Ok(ndx) => ndx,
                        // Otherwise, if ndx is not a number (e.g., "ABS"), then we just skip that entry (go onto the next line). 
                        _ => {
                            trace!("parse_nano_core_symbol_file(): skipping line {}: {}", _line_num + 1, line);
                            continue;
                        }
                    };

                    // debug!("parse_nano_core_symbol_file(): name: {}, vaddr: {:#X}, size: {:#X}, sec_ndx {}", name, sec_vaddr, sec_size, sec_ndx);

                    if sec_ndx == text_shndx {
                        sections.insert(
                            section_counter,
                            Arc::new(Mutex::new(LoadedSection::new(
                                SectionType::Text,
                                name.to_string(),
                                Arc::clone(&text_pages),
                                try_break!(text_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core text section wasn't covered by its mapped pages!"), loop_result), 
                                sec_vaddr,
                                sec_size,
                                true,
                                new_crate_weak_ref.clone(), 
                            )))
                        );
                    }
                    else if sec_ndx == rodata_shndx {
                        sections.insert(
                            section_counter,
                            Arc::new(Mutex::new(LoadedSection::new(
                                SectionType::Rodata,
                                name.to_string(),
                                Arc::clone(&rodata_pages),
                                try_break!(rodata_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core rodata section wasn't covered by its mapped pages!"), loop_result),
                                sec_vaddr,
                                sec_size,
                                true,
                                new_crate_weak_ref.clone(),
                            )))
                        );
                    }
                    else if sec_ndx == data_shndx {
                        sections.insert(
                            section_counter,
                            Arc::new(Mutex::new(LoadedSection::new(
                                SectionType::Data,
                                name.to_string(),
                                Arc::clone(&data_pages),
                                try_break!(data_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core data section wasn't covered by its mapped pages!"), loop_result),
                                sec_vaddr,
                                sec_size,
                                true,
                                new_crate_weak_ref.clone(),
                            )))
                        );
                    }
                    else if sec_ndx == bss_shndx {
                        sections.insert(
                            section_counter,
                            Arc::new(Mutex::new(LoadedSection::new(
                                SectionType::Bss,
                                name.to_string(),
                                Arc::clone(&data_pages),
                                try_break!(data_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core bss section wasn't covered by its mapped pages!"), loop_result),
                                sec_vaddr,
                                sec_size,
                                true,
                                new_crate_weak_ref.clone(),
                            )))
                        );
                    }
                    else {
                        trace!("parse_nano_core_symbol_file(): skipping sec[{}] (probably in .init): name: {}, vaddr: {:#X}, size: {:#X}", sec_ndx, name, sec_vaddr, sec_size);
                    }

                    section_counter += 1;

                } // end of loop over all lines
            }

            // check to see if we had an error in the above loop
            try_mp!(loop_result, text_pages, rodata_pages, data_pages);
            trace!("parse_nano_core_symbol_file(): finished looping over symtab.");

        } // drops the borrow of `bytes` (and mapped_pages)
        
        // set the new_crate's sections list, since we didn't do it earlier
        {
            let mut new_crate_mut = try_mp!(
                new_crate.lock_as_mut()
                    .ok_or_else(|| "BUG: parse_nano_core_symbol_file(): couldn't get exclusive mutable access to new_crate"),
                text_pages, rodata_pages, data_pages
            );
            new_crate_mut.sections = sections;
        }
        
        Ok(new_crate)
    }




    /// Parses the nano_core ELF binary file, which is already loaded and running.  
    /// Thus, we simply search for its global symbols, and add them to the system map and the crate metadata.
    /// 
    /// Drops the given `mapped_pages` that hold the nano_core binary file itself.
    fn parse_nano_core_binary(
        mapped_pages: MappedPages, 
        nano_core_object_file: &'static ModuleArea,
        text_pages:   Arc<Mutex<MappedPages>>, 
        rodata_pages: Arc<Mutex<MappedPages>>, 
        data_pages:   Arc<Mutex<MappedPages>>, 
        size_in_bytes: usize
    ) -> Result<StrongCrateRef, (&'static str, [Arc<Mutex<MappedPages>>; 3])> {
        let crate_name = String::from("nano_core");
        debug!("Parsing {} binary: size {:#x}({}), MappedPages: {:?}, text_pages: {:?}, rodata_pages: {:?}, data_pages: {:?}", 
                crate_name, size_in_bytes, size_in_bytes, mapped_pages, text_pages, rodata_pages, data_pages);

        let byte_slice: &[u8] = try_mp!(mapped_pages.as_slice(0, size_in_bytes), text_pages, rodata_pages, data_pages);
        let elf_file = try_mp!(ElfFile::new(byte_slice), text_pages, rodata_pages, data_pages); // returns Err(&str) if ELF parse fails

        // For us to properly load the ELF file, it must NOT have been stripped,
        // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
        use xmas_elf::sections::SectionData::SymbolTable64;
        let sssec = elf_file.section_iter().filter(|sec| sec.get_type() == Ok(ShType::SymTab)).next();
        let symtab_data = match sssec.ok_or("no symtab section").and_then(|s| s.get_data(&elf_file)) {
            Ok(SymbolTable64(symtab)) => Ok(symtab),
            _ => {
                error!("parse_nano_core_binary(): can't load file: no symbol table found. Was file stripped?");
                Err("cannot load nano_core: no symbol table found. Was file stripped?")
            }
        };
        let symtab = try_mp!(symtab_data, text_pages, rodata_pages, data_pages);
        // debug!("symtab: {:?}", symtab);
        
        // find the .text, .data, and .rodata sections
        let mut text_shndx:   Option<usize> = None;
        let mut rodata_shndx: Option<usize> = None;
        let mut data_shndx:   Option<usize> = None;
        let mut bss_shndx:    Option<usize> = None;

        let mut loop_result: Result<(), &'static str> = Ok(());
        
        for (shndx, sec) in elf_file.section_iter().enumerate() {
            // trace!("parse_nano_core_binary(): looking at sec[{}]: {:?}", shndx, sec);
            // the PROGBITS sections are the bulk of what we care about, i.e., .text & data sections
            if let Ok(ShType::ProgBits) = sec.get_type() {
                // skip null section and any empty sections
                let sec_size = sec.size() as usize;
                if sec_size == 0 { continue; }

                if let Ok(name) = sec.get_name(&elf_file) {
                    match name {
                        ".text" => {
                            if !(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC | SHF_EXECINSTR)) {
                                try_break!(Err(".text section had wrong flags!"), loop_result);
                            }
                            text_shndx = Some(shndx);
                        }
                        ".data" => {
                            if !(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC | SHF_WRITE)) {
                                try_break!(Err(".data section had wrong flags!"), loop_result);
                            }
                            data_shndx = Some(shndx);
                        }
                        ".rodata" => {
                            if !(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC)) {
                                try_break!(Err(".rodata section had wrong flags!"), loop_result);
                            }
                            rodata_shndx = Some(shndx);
                        }
                        _ => {
                            continue;
                        }
                    };
                }
            }
            // look for .bss section
            else if let Ok(ShType::NoBits) = sec.get_type() {
                // skip null section and any empty sections
                let sec_size = sec.size() as usize;
                if sec_size == 0 { continue; }

                if let Ok(name) = sec.get_name(&elf_file) {
                    if name == ".bss" {
                        if !(sec.flags() & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) == (SHF_ALLOC | SHF_WRITE)) {
                            try_break!(Err(".bss section had wrong flags!"), loop_result);
                        }
                        bss_shndx = Some(shndx);
                    }
                }
            }

            // // once we've found the 4 sections we care about, skip the rest.
            // if text_shndx.is_some() && rodata_shndx.is_some() && data_shndx.is_some() && bss_shndx.is_some() {
            //     break;
            // }
        }

        try_mp!(loop_result, text_pages, rodata_pages, data_pages);

        let text_shndx   = try_mp!(text_shndx.ok_or("couldn't find .text section in nano_core ELF"), text_pages, rodata_pages, data_pages);
        let rodata_shndx = try_mp!(rodata_shndx.ok_or("couldn't find .rodata section in nano_core ELF"), text_pages, rodata_pages, data_pages);
        let data_shndx   = try_mp!(data_shndx.ok_or("couldn't find .data section in nano_core ELF"), text_pages, rodata_pages, data_pages);
        let bss_shndx    = try_mp!(bss_shndx.ok_or("couldn't find .bss section in nano_core ELF"), text_pages, rodata_pages, data_pages);

        let new_crate = CowArc::new(LoadedCrate {
            crate_name:              crate_name, 
            object_file:             nano_core_object_file,
            sections:                BTreeMap::new(),
            text_pages:              Some(text_pages.clone()),
            rodata_pages:            Some(rodata_pages.clone()),
            data_pages:              Some(data_pages.clone()),
            global_symbols:          BTreeSet::new(),
            bss_sections:            Trie::new(),
            reexported_symbols:      BTreeSet::new(),
        });
        let new_crate_weak_ref = CowArc::downgrade(&new_crate);

        let mut loop_result: Result<(), &'static str> = Ok(());
        
        let sections = {
            let text_pages_locked = text_pages.lock();
            let rodata_pages_locked = rodata_pages.lock();
            let data_pages_locked = data_pages.lock();

            // Iterate through the symbol table so we can find which sections are global (publicly visible).
            // As the nano_core doesn't have one section per function/data/rodata, we fake it here with an arbitrary section counter.
            let mut section_counter = 0;
            let mut sections: BTreeMap<usize, StrongSectionRef> = BTreeMap::new();

            use xmas_elf::symbol_table::Entry;
            for entry in symtab.iter() {
                // public symbols can have any visibility setting, but it's the binding that matters (must be GLOBAL)

                // use xmas_elf::symbol_table::Visibility;
                // match entry.get_other() {
                //     Visibility::Default | Visibility::Hidden => {
                //         // do nothing, fall through to proceed
                //     }
                //     _ => {
                //         continue; // skip this
                //     }
                // };
                
                if let Ok(bind) = entry.get_binding() {
                    if bind == xmas_elf::symbol_table::Binding::Global {
                        if let Ok(typ) = entry.get_type() {
                            if typ == xmas_elf::symbol_table::Type::Func || typ == xmas_elf::symbol_table::Type::Object {
                                let sec_vaddr = entry.value() as VirtualAddress;
                                let sec_size = entry.size() as usize;
                                let name = try_break!(entry.get_name(&elf_file), loop_result);

                                let demangled = demangle(name).to_string();
                                // debug!("parse_nano_core_binary(): name: {}, demangled: {}, vaddr: {:#X}, size: {:#X}", name, demangled, sec_vaddr, sec_size);

                                let new_section = {
                                    if entry.shndx() as usize == text_shndx {
                                        Some(LoadedSection::new(
                                            SectionType::Text,
                                            demangled,
                                            Arc::clone(&text_pages),
                                            try_break!(text_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core text section wasn't covered by its mapped pages!"), loop_result),
                                            sec_vaddr,
                                            sec_size,
                                            true,
                                            new_crate_weak_ref.clone(),
                                        ))
                                    }
                                    else if entry.shndx() as usize == rodata_shndx {
                                        Some(LoadedSection::new(
                                            SectionType::Rodata,
                                            demangled,
                                            Arc::clone(&rodata_pages),
                                            try_break!(rodata_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core rodata section wasn't covered by its mapped pages!"), loop_result),
                                            sec_vaddr,
                                            sec_size,
                                            true,
                                            new_crate_weak_ref.clone(),
                                        ))
                                    }
                                    else if entry.shndx() as usize == data_shndx {
                                        Some(LoadedSection::new(
                                            SectionType::Data,
                                            demangled,
                                            Arc::clone(&data_pages),
                                            try_break!(data_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core data section wasn't covered by its mapped pages!"), loop_result),
                                            sec_vaddr,
                                            sec_size,
                                            true,
                                            new_crate_weak_ref.clone(),
                                        ))
                                    }
                                    else if entry.shndx() as usize == bss_shndx {
                                        Some(LoadedSection::new(
                                            SectionType::Bss,
                                            demangled,
                                            Arc::clone(&data_pages),
                                            try_break!(data_pages_locked.offset_of_address(sec_vaddr).ok_or("nano_core bss section wasn't covered by its mapped pages!"), loop_result),
                                            sec_vaddr,
                                            sec_size,
                                            true,
                                            new_crate_weak_ref.clone(),
                                        ))
                                    }
                                    else {
                                        error!("Unexpected entry.shndx(): {}", entry.shndx());
                                        None
                                    }
                                };

                                if let Some(sec) = new_section {
                                    // debug!("parse_nano_core: new section: {:?}", sec);
                                    sections.insert(section_counter, Arc::new(Mutex::new(sec)));
                                    section_counter += 1;
                                }
                            }
                        }
                    }
                }
            }
            sections 
        };

        // check if there was an error in the loop above
        try_mp!(loop_result, text_pages, rodata_pages, data_pages);

        // set the new_crate's sections list, since we didn't do it earlier
        {
            let mut new_crate_mut = try_mp!(
                new_crate.lock_as_mut()
                    .ok_or_else(|| "BUG: parse_nano_core_binary(): couldn't get exclusive mutable access to new_crate"),
                text_pages, rodata_pages, data_pages
            );
            new_crate_mut.sections = sections;
        }

        Ok(new_crate)
    }
}
pub mod metadata {
    //! This metadata module contains metadata about all other modules/crates loaded in Theseus.
    //! 
    //! [This is a good link](https://users.rust-lang.org/t/circular-reference-issue/9097)
    //! for understanding why we need `Arc`/`Weak` to handle recursive/circular data structures in Rust. 

    use core::ops::{Deref};
    use spin::Mutex;
    use alloc::vec::Vec;
    use alloc::string::String;
    use alloc::collections::{BTreeMap, BTreeSet};
    use alloc::sync::{Arc, Weak};
    use memory::{MappedPages, ModuleArea, VirtualAddress, PageTable, MemoryManagementInfo, EntryFlags, FrameAllocator};
    use dependency::*;
    use super::{TEXT_SECTION_FLAGS, RODATA_SECTION_FLAGS, DATA_BSS_SECTION_FLAGS};
    use cow_arc::{CowArc, CowWeak};

    use super::SymbolMap;
    use qp_trie::{Trie, wrapper::BString};

    /// A Strong reference (`Arc`) to a `LoadedSection`.
    pub type StrongSectionRef  = Arc<Mutex<LoadedSection>>;
    /// A Weak reference (`Weak`) to a `LoadedSection`.
    pub type WeakSectionRef = Weak<Mutex<LoadedSection>>;

    /// A Strong reference (`Arc`) to a `LoadedCrate`.
    pub type StrongCrateRef  = CowArc<LoadedCrate>; // Arc<Mutex<LoadedCrate>>;
    /// A Weak reference (`Weak`) to a `LoadedCrate`.
    pub type WeakCrateRef = CowWeak<LoadedCrate>; // Weak<Mutex<LoadedCrate>>;


    const CRATE_PREFIX_DELIMITER: &'static str = "#";

    #[derive(PartialEq)]
    pub enum CrateType {
        Kernel,
        Application,
        Userspace,
    }
    impl CrateType {
        pub fn prefix(&self) -> &'static str {
            match self {
                CrateType::Kernel       => "k#",
                CrateType::Application  => "a#",
                CrateType::Userspace    => "u#",
            }
        }

        fn first_prefix(&self) -> &'static str {
            match self {
                CrateType::Kernel       => "k",
                CrateType::Application  => "a",
                CrateType::Userspace    => "u",
            }
        }

        /// Returns a tuple of (CrateType, &str) based on the given `module_name`,
        /// in which the `&str` is the rest of the module name after the prefix. 
        /// # Examples 
        /// ```
        /// let result = CrateType::from_module_name("k#my_crate");
        /// assert_eq!(result, (CrateType::Kernel, "my_crate") );
        /// ```
        pub fn from_module_name<'a>(module_name: &'a str) -> Result<(CrateType, &'a str), &'static str> {
            let mut iter = module_name.split(CRATE_PREFIX_DELIMITER);
            let prefix = iter.next().ok_or("couldn't get crate type prefix before delimiter")?;
            let crate_name = iter.next().ok_or("couldn't get crate name after prefix delimiter")?;
            if iter.next().is_some() {
                return Err("too many # delimiters in crate's module name");
            }
            
            if prefix.starts_with(CrateType::Kernel.first_prefix()) {
                Ok((CrateType::Kernel, crate_name))
            }
            else if prefix.starts_with(CrateType::Application.first_prefix()) {
                Ok((CrateType::Application, crate_name))
            }
            else if prefix.starts_with(CrateType::Userspace.first_prefix()) {
                Ok((CrateType::Userspace, crate_name))
            }
            else {
                Err("module_name didn't start with a known CrateType prefix")
            }
        }


        pub fn is_application(module_name: &str) -> bool {
            module_name.starts_with(CrateType::Application.first_prefix())
        }

        pub fn is_kernel(module_name: &str) -> bool {
            module_name.starts_with(CrateType::Kernel.first_prefix())
        }

        pub fn is_userspace(module_name: &str) -> bool {
            module_name.starts_with(CrateType::Userspace.first_prefix())
        }
    }


    /// Represents a single crate object file that has been loaded into the system.
    pub struct LoadedCrate {
        /// The name of this crate.
        pub crate_name: String,
        /// The unique identifier of the object file that this crate was loaded from.
        /// Currently, this is a reference to the corresponding `ModuleArea`.
        pub object_file: &'static ModuleArea,
        /// A map containing all the sections in this crate.
        /// In general we're only interested the values (the `LoadedSection`s themselves),
        /// but we keep each section's shndx (section header index from its crate's ELF file)
        /// as the key because it helps us quickly handle relocations and crate swapping.
        pub sections: BTreeMap<usize, StrongSectionRef>,
        /// The `MappedPages` that include the text sections for this crate,
        /// i.e., sections that are readable and executable, but not writable.
        pub text_pages: Option<Arc<Mutex<MappedPages>>>,
        /// The `MappedPages` that include the rodata sections for this crate.
        /// i.e., sections that are read-only, not writable nor executable.
        pub rodata_pages: Option<Arc<Mutex<MappedPages>>>,
        /// The `MappedPages` that include the data and bss sections for this crate.
        /// i.e., sections that are readable and writable but not executable.
        pub data_pages: Option<Arc<Mutex<MappedPages>>>,
        
        // The members below are most used to accelerate crate swapping //

        /// The set of global symbols in this crate, including regular ones 
        /// that are prefixed with the `crate_name` and `no_mangle` symbols that are not.
        pub global_symbols: BTreeSet<BString>,
        /// The set of BSS sections in this crate.
        /// The key is the section name and the value is a reference to the section;
        /// these sections are also in the `sections` member above.
        pub bss_sections: Trie<BString, StrongSectionRef>,
        /// The set of symbols that this crate's global symbols are reexported under,
        /// i.e., they have been added to the enclosing `CrateNamespace`'s symbol map under these names.
        /// 
        /// This is primarily used when swapping crates, and it is useful in the following way. 
        /// If this crate is the new crate that is swapped in to replace another crate, 
        /// and the caller of the `swap_crates()` function specifies that this crate 
        /// should expose its symbols with names that match the old crate it's replacing, 
        /// then this will be populated with the names of corresponding symbols from the old crate that its replacing.
        /// For example, if this crate has a symbol `keyboard::init::h456`, and it replaced an older crate
        /// that had the symbol `keyboard::init::123`, and `reexport_new_symbols_as_old` was true,
        /// then `keyboard::init::h123` will be added to this set.
        /// 
        /// When a crate is first loaded, this will be empty by default, 
        /// because this crate will only have populated its `global_symbols` set during loading. 
        pub reexported_symbols: BTreeSet<BString>,
    }

    use core::fmt;
    impl fmt::Debug for LoadedCrate {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "LoadedCrate(name: {:?}, objfile: {:?})", self.crate_name, self.object_file)
        }
    }

    impl Drop for LoadedCrate {
        fn drop(&mut self) {
            trace!("### Dropped LoadedCrate: {}", self.crate_name);
        }
    }

    impl LoadedCrate {
        /// Returns the `LoadedSection` of type `SectionType::Text` that matches the requested function name, if it exists in this `LoadedCrate`.
        /// Only matches demangled names, e.g., "my_crate::foo".
        pub fn get_function_section(&self, func_name: &str) -> Option<&StrongSectionRef> {
            self.find_section(|sec| sec.is_text() && sec.name == func_name)
        }

        /// Returns the first `LoadedSection` that matches the given predicate
        pub fn find_section<F>(&self, predicate: F) -> Option<&StrongSectionRef> 
            where F: Fn(&LoadedSection) -> bool
        {
            self.sections.values()
                .filter(|sec_ref| predicate(&sec_ref.lock()))
                .next()
        }

        /// Returns the substring of this crate's name that excludes the trailing hash. 
        /// If there is no hash, then it returns the entire name. 
        pub fn crate_name_without_hash(&self) -> &str {
            // the hash identifier (delimiter) is "-"
            self.crate_name.split("-")
                .next()
                .unwrap_or_else(|| &self.crate_name)
        }

        /// Returns this crate name as a symbol prefix, including a trailing "`::`".
        /// If there is no hash, then it returns the entire name with a trailing "`::`".
        /// # Example
        /// * Crate name: "`driver_init-e3769b63863a4030`", return value: "`driver_init::`"
        /// * Crate name: "`hello`"` return value: "`hello::`"
        pub fn crate_name_as_prefix(&self) -> String {
            format!("{}::", self.crate_name_without_hash())
        }

        /// Currently may contain duplicates!
        pub fn crates_dependent_on_me(&self) -> Vec<WeakCrateRef> {
            let mut results: Vec<WeakCrateRef> = Vec::new();
            for sec in self.sections.values() {
                let sec_locked = sec.lock();
                for dep_sec in &sec_locked.sections_dependent_on_me {
                    if let Some(dep_sec) = dep_sec.section.upgrade() {
                        let dep_sec_locked = dep_sec.lock();
                        let parent_crate = dep_sec_locked.parent_crate.clone();
                        results.push(parent_crate);
                    }
                }
            }
            results
        }

        /// Creates a new copy of this `LoadedCrate`, which is a relatively slow process
        /// because it must do the following:    
        /// * Deep copy all of the MappedPages into completely new memory regions.
        /// * Duplicate every section within this crate.
        /// * Recalculate every relocation entry to point to the newly-copied sections,
        ///   which is the most time-consuming component of this function.
        /// 
        /// # Notes
        /// This is obviously different from cloning a shared Arc reference to this `LoadedCrate`,
        /// i.e., a `StrongCrateRef`, which is an instant and cheap operation that does not duplicate the underlying `LoadedCrate`.
        /// 
        /// Also, there is currently no way to deep copy a single `LoadedSection` in isolation,
        /// because a single section has dependencies on many other sections, i.e., due to relocations,
        /// and that would result in weird inconsistencies that violate those dependencies.
        /// In addition, multiple `LoadedSection`s share a given `MappedPages` memory range,
        /// so they all have to be duplicated at once into a new `MappedPages` range at the crate level.
        pub fn deep_copy<A: FrameAllocator>(
            &self, 
            kernel_mmi: &mut MemoryManagementInfo, 
            allocator: &mut A
        ) -> Result<StrongCrateRef, &'static str> {
            // First, deep copy all of the memory regions.
            // We initially map the as writable because we'll have to copy things into them
            let (new_text_pages, new_rodata_pages, new_data_pages) = {
                if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                    let new_text_pages = match self.text_pages {
                        Some(ref tp) => Some(tp.lock().deep_copy(Some(TEXT_SECTION_FLAGS() | EntryFlags::WRITABLE), active_table, allocator)?),
                        None => None,
                    };
                    let new_rodata_pages = match self.rodata_pages {
                        Some(ref rp) => Some(rp.lock().deep_copy(Some(RODATA_SECTION_FLAGS() | EntryFlags::WRITABLE), active_table, allocator)?),
                        None => None,
                    };
                    let new_data_pages = match self.data_pages {
                        Some(ref dp) => Some(dp.lock().deep_copy(Some(DATA_BSS_SECTION_FLAGS()), active_table, allocator)?),
                        None => None,
                    };
                    (new_text_pages, new_rodata_pages, new_data_pages)
                }
                else {
                    return Err("couldn't get kernel's active page table");
                }
            };

            let new_text_pages_ref   = new_text_pages  .map(|mp| Arc::new(Mutex::new(mp)));
            let new_rodata_pages_ref = new_rodata_pages.map(|mp| Arc::new(Mutex::new(mp)));
            let new_data_pages_ref   = new_data_pages  .map(|mp| Arc::new(Mutex::new(mp)));

            let mut new_text_pages_locked   = new_text_pages_ref  .as_ref().map(|tp| tp.lock());
            let mut new_rodata_pages_locked = new_rodata_pages_ref.as_ref().map(|rp| rp.lock());
            let mut new_data_pages_locked   = new_data_pages_ref  .as_ref().map(|dp| dp.lock());

            let new_crate = CowArc::new(LoadedCrate {
                crate_name:              self.crate_name.clone(),
                object_file:             self.object_file,
                sections:                BTreeMap::new(),
                text_pages:              new_text_pages_ref.clone(),
                rodata_pages:            new_rodata_pages_ref.clone(),
                data_pages:              new_data_pages_ref.clone(),
                global_symbols:          self.global_symbols.clone(),
                bss_sections:            Trie::new(),
                reexported_symbols:      self.reexported_symbols.clone(),
            });
            let new_crate_weak_ref = CowArc::downgrade(&new_crate);

            // Second, deep copy the entire list of sections and fix things that don't make sense to directly clone:
            // 1) The parent_crate reference itself, since we're replacing that with a new one,
            // 2) The section's mapped_pages, which will point to a new `MappedPages` object for the newly-copied crate,
            // 3) The section's virt_addr, which is based on its new mapped_pages
            let mut new_sections: BTreeMap<usize, StrongSectionRef> = BTreeMap::new();
            let mut new_bss_sections: Trie<BString, StrongSectionRef> = Trie::new();
            for (shndx, old_sec_ref) in self.sections.iter() {
                let old_sec = old_sec_ref.lock();
                let new_sec_mapped_pages_offset = old_sec.mapped_pages_offset;
                let (new_sec_mapped_pages_ref, new_sec_virt_addr) = match old_sec.typ {
                    SectionType::Text => (
                        new_text_pages_ref.clone().ok_or_else(|| "BUG: missing text pages in newly-copied crate")?,
                        new_text_pages_locked.as_ref().and_then(|tp| tp.address_at_offset(new_sec_mapped_pages_offset)),
                    ),
                    SectionType::Rodata => (
                        new_rodata_pages_ref.clone().ok_or_else(|| "BUG: missing rodata pages in newly-copied crate")?,
                        new_rodata_pages_locked.as_ref().and_then(|rp| rp.address_at_offset(new_sec_mapped_pages_offset)),
                    ),
                    SectionType::Data |
                    SectionType::Bss => (
                        new_data_pages_ref.clone().ok_or_else(|| "BUG: missing data pages in newly-copied crate")?,
                        new_data_pages_locked.as_ref().and_then(|dp| dp.address_at_offset(new_sec_mapped_pages_offset)),
                    ),
                };
                let new_sec_virt_addr = new_sec_virt_addr.ok_or_else(|| "BUG: couldn't get virt_addr for new section")?;

                let new_sec_ref = Arc::new(Mutex::new(LoadedSection::with_dependencies(
                    old_sec.typ,                            // section type is the same
                    old_sec.name.clone(),                   // name is the same
                    new_sec_mapped_pages_ref,               // mapped_pages is different, points to the new duplicated one
                    new_sec_mapped_pages_offset,            // mapped_pages_offset is the same
                    new_sec_virt_addr,                      // virt_addr is different, based on the new mapped_pages
                    old_sec.size,                           // size is the same
                    old_sec.global,                         // globalness is the same
                    new_crate_weak_ref.clone(),             // parent_crate is different, points to the newly-copied crate
                    old_sec.sections_i_depend_on.clone(),   // dependencies are the same, but relocations need to be re-written
                    Vec::new(),                             // no sections can possibly depend on this one, since we just created it
                    old_sec.internal_dependencies.clone()   // internal dependencies are the same, but relocations need to be re-written
                )));

                if old_sec.typ == SectionType::Bss {
                    new_bss_sections.insert_str(&old_sec.name, new_sec_ref.clone());
                }
                new_sections.insert(*shndx, new_sec_ref);
            }


            // Now we can go through the list again and fix up the rest of the elements in each section.
            // The foreign sections dependencies (sections_i_depend_on) are the same, 
            // but all relocation entries must be rewritten because the sections' virtual addresses have changed.
            for new_sec_ref in new_sections.values() {
                let mut new_sec = new_sec_ref.lock();
                let new_sec_mapped_pages = match new_sec.typ {
                    SectionType::Text   => new_text_pages_locked.as_mut().ok_or_else(|| "BUG: missing text pages in newly-copied crate")?,
                    SectionType::Rodata => new_rodata_pages_locked.as_mut().ok_or_else(|| "BUG: missing rodata pages in newly-copied crate")?,
                    SectionType::Data |
                    SectionType::Bss    => new_data_pages_locked.as_mut().ok_or_else(|| "BUG: missing data pages in newly-copied crate")?,
                };
                let new_sec_mapped_pages_offset = new_sec.mapped_pages_offset;

                // The newly-duplicated crate still depends on the same sections, so we keep those as is, 
                // but we do need to recalculate those relocations.
                for mut strong_dep in new_sec.sections_i_depend_on.iter_mut() {
                    // we can skip modifying "absolute" relocations, since those only depend on the source section,
                    // which we haven't actually changed (we've duplicated the target section here, not the source)
                    if !strong_dep.relocation.is_absolute() {
                        let mut source_sec = strong_dep.section.lock();
                        // perform the actual fix by writing the relocation
                        super::write_relocation(
                            strong_dep.relocation, 
                            new_sec_mapped_pages, 
                            new_sec_mapped_pages_offset,
                            source_sec.virt_addr,
                            true
                        )?;

                        // add this new_sec as one of the source sec's weak dependents
                        source_sec.sections_dependent_on_me.push(
                            WeakDependent {
                                section: Arc::downgrade(new_sec_ref),
                                relocation: strong_dep.relocation,
                            }
                        );
                    }
                }

                // Finally, fix up all of its internal dependencies by recalculating/rewriting their relocations.
                // We shouldn't need to actually change the InternalDependency instances themselves 
                // because they are based on crate-specific section shndx values, 
                // which are completely safe to clone without needing any fix ups. 
                for internal_dep in &new_sec.internal_dependencies {
                    let source_sec_ref = new_sections.get(&internal_dep.source_sec_shndx)
                        .ok_or_else(|| "Couldn't get new section specified by an internal dependency's source_sec_shndx")?;

                    // The source and target (new_sec) sections might be the same, so we need to check first
                    // to ensure that we don't cause deadlock by trying to lock the same section twice.
                    let source_sec_vaddr = if Arc::ptr_eq(source_sec_ref, new_sec_ref) {
                        // here: the source_sec and new_sec are the same, so just use the already-locked new_sec
                        new_sec.virt_addr()
                    } else {
                        // here: the source_sec and new_sec are different, so we can go ahead and safely lock the source_sec
                        source_sec_ref.lock().virt_addr()
                    };
                    super::write_relocation(
                        internal_dep.relocation, 
                        new_sec_mapped_pages, 
                        new_sec_mapped_pages_offset,
                        source_sec_vaddr,
                        true
                    )?;
                }
            }

            // since we mapped all the new MappedPages as writable, we need to properly remap them
            if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                if let Some(ref mut tp) = new_text_pages_locked { 
                    try!(tp.remap(active_table, TEXT_SECTION_FLAGS()));
                }
                if let Some(ref mut rp) = new_rodata_pages_locked { 
                    try!(rp.remap(active_table, RODATA_SECTION_FLAGS()));
                }
                // data/bss sections are already mapped properly, since they're supposed to be writable
            }
            else {
                return Err("couldn't get kernel's active page table");
            }

            // set the new_crate's section-related lists, since we didn't do it earlier
            {
                let mut new_crate_mut = new_crate.lock_as_mut()
                    .ok_or_else(|| "BUG: LoadedCrate::deep_copy(): couldn't get exclusive mutable access to copied new_crate")?;
                new_crate_mut.sections = new_sections;
                new_crate_mut.bss_sections = new_bss_sections;
            }

            Ok(new_crate)
        }
    }


    /// Returns a map containing all symbols,
    /// filtered to include only `LoadedSection`s that satisfy the given predicate
    /// (if the predicate returns true for a given section, then it is included in the map).
    /// 
    /// The symbols come from the sections in the given section iterator (if Some), 
    /// otherwise they come from this crate's sections list.
    /// 
    /// See [`global_symbol_map`](#method.global_system_map) for an example.
    pub fn symbol_map<'a, I, F>(
        sections: I,
        crate_name: &str,
        predicate: F,
    ) -> SymbolMap 
        where F: Fn(&LoadedSection) -> bool, 
                I: IntoIterator<Item = &'a StrongSectionRef> 
    {
        let mut map: SymbolMap = SymbolMap::new();
        for sec in sections.into_iter().filter(|sec| predicate(sec.lock().deref())) {
            let key = sec.lock().name.clone();
            if let Some(old_val) = map.insert_str(&key, Arc::downgrade(&sec)) {
                if key.ends_with("_LOC") || crate_name == "nano_core" {
                    // ignoring these special cases currently
                }
                else {
                    warn!("symbol_map(): crate \"{}\" had duplicate section for symbol \"{}\", old: {:?}, new: {:?}", 
                        crate_name, key, old_val.upgrade(), sec);
                }
            }
        }

        map
    }


    /// The possible types of `LoadedSection`s: .text, .rodata, .data, or .bss.
    /// A .bss section is basically treated the same as .data, 
    /// but we keep them separate
    #[derive(Debug, Copy, Clone, PartialEq)]
    pub enum SectionType {
        Text,
        Rodata,
        Data,
        Bss
    }

    /// Represents a .text, .rodata, .data, or .bss section
    /// that has been loaded and is part of a `LoadedCrate`.
    /// The containing `SectionType` enum determines which type of section it is.
    pub struct LoadedSection {
        /// The type of this section: .text, .rodata, .data, or .bss.
        pub typ: SectionType,
        /// The full String name of this section, a fully-qualified symbol, 
        /// with the format `<crate>::[<module>::][<struct>::]<fn_name>::<hash>`.
        /// The unique hash is generated for each section by the Rust compiler,
        /// which can be used as a version identifier. 
        /// Not all symbols will have a hash, e.g., ones that are not mangled.
        /// 
        /// # Examples
        /// * `test_lib::MyStruct::new::h843a613894da0c24`
        /// * `my_crate::my_function::hbce878984534ceda`   
        pub name: String,
        /// The `MappedPages` that cover this section.
        pub mapped_pages: Arc<Mutex<MappedPages>>, 
        /// The offset into the `mapped_pages` where this section starts
        pub mapped_pages_offset: usize,
        /// The `VirtualAddress` of this section, cached here as a performance optimization
        /// so we can avoid doing the calculation based on this section's mapped_pages and mapped_pages_offset.
        /// This address value should not be used for accessing this section's data through 
        /// a non-safe dereference or transmute operation. 
        /// Instead, it's just here to help speed up and simply relocations.
        virt_addr: VirtualAddress, 
        /// The size in bytes of this section
        pub size: usize,
        /// Whether or not this section's symbol was exported globally (is public)
        pub global: bool,
        /// The `LoadedCrate` object that contains/owns this section
        pub parent_crate: WeakCrateRef,
        /// The list of sections in foreign crates that this section depends on, i.e., "my required dependencies".
        /// This is kept as a list of strong references because these sections must outlast this section,
        /// i.e., those sections cannot be removed/deleted until this one is deleted.
        pub sections_i_depend_on: Vec<StrongDependency>,
        /// The list of sections in foreign crates that depend on this section, i.e., "my dependents".
        /// This is kept as a list of Weak references because we must be able to remove other sections
        /// that are dependent upon this one before we remove this one.
        /// If we kept strong references to the sections dependent on this one, 
        /// then we wouldn't be able to remove/delete those sections before deleting this one.
        pub sections_dependent_on_me: Vec<WeakDependent>,
        /// We keep track of inter-section dependencies within the same crate
        /// so that we can faithfully reconstruct the crate section's relocation information.
        /// This is necessary for doing a deep copy of the crate in memory, 
        /// without having to re-parse that crate's ELF file (and requiring the ELF file to still exist).
        pub internal_dependencies: Vec<InternalDependency>,
    }
    impl LoadedSection {
        /// Create a new `LoadedSection`, with an empty `dependencies` list.
        pub fn new(
            typ: SectionType, 
            name: String, 
            mapped_pages: Arc<Mutex<MappedPages>>,
            mapped_pages_offset: usize,
            virt_addr: VirtualAddress,
            size: usize,
            global: bool, 
            parent_crate: WeakCrateRef,
        ) -> LoadedSection {
            LoadedSection::with_dependencies(typ, name, mapped_pages, mapped_pages_offset, virt_addr, size, global, parent_crate, Vec::new(), Vec::new(), Vec::new())
        }

        /// Same as [new()`](#method.new), but uses the given `dependencies` instead of the default empty list.
        pub fn with_dependencies(
            typ: SectionType, 
            name: String, 
            mapped_pages: Arc<Mutex<MappedPages>>,
            mapped_pages_offset: usize,
            virt_addr: VirtualAddress,
            size: usize,
            global: bool, 
            parent_crate: WeakCrateRef,
            sections_i_depend_on: Vec<StrongDependency>,
            sections_dependent_on_me: Vec<WeakDependent>,
            internal_dependencies: Vec<InternalDependency>,
        ) -> LoadedSection {
            LoadedSection {
                typ, name, mapped_pages, mapped_pages_offset, virt_addr, size, global, parent_crate, sections_i_depend_on, sections_dependent_on_me, internal_dependencies
            }
        }

        /// Returns the starting `VirtualAddress` of where this section is loaded into memory. 
        pub fn virt_addr(&self) -> VirtualAddress {
            self.virt_addr
        }

        /// Whether this `LoadedSection` is a .text section
        pub fn is_text(&self) -> bool {
            self.typ == SectionType::Text
        }

        /// Whether this `LoadedSection` is a .rodata section
        pub fn is_rodata(&self) -> bool {
            self.typ == SectionType::Rodata
        }

        /// Whether this `LoadedSection` is a .data section
        pub fn is_data(&self) -> bool {
            self.typ == SectionType::Data
        }

        /// Whether this `LoadedSection` is a .bss section
        pub fn is_bss(&self) -> bool {
            self.typ == SectionType::Bss
        }

        /// Returns the substring of this section's name that excludes the trailing hash,
        /// but includes the hash delimiter "`::h`". 
        /// If there is no hash, then it returns the entire name. 
        /// 
        /// # Examples
        /// name: "`keyboard_new::init::h832430094f98e56b`", return value: "`keyboard_new::init::h`"
        /// name: "`start_me`", return value: "`start_me`"
        pub fn name_without_hash(&self) -> &str {
            // the hash identifier (delimiter) is "::h"
            const HASH_DELIMITER: &'static str = "::h";
            self.name.rfind("::h")
                .and_then(|end| self.name.get(0 .. (end + HASH_DELIMITER.len())))
                .unwrap_or_else(|| &self.name)
        }

        /// Returns the index of the first `WeakDependent` object with a section
        /// that matches the given `matching_section` in this `LoadedSection`'s `sections_dependent_on_me` list.
        pub fn find_weak_dependent(&self, matching_section: &StrongSectionRef) -> Option<usize> {
            for (index, weak_dep) in self.sections_dependent_on_me.iter().enumerate() {
                if let Some(sec_ref) = weak_dep.section.upgrade() {
                    if Arc::ptr_eq(matching_section, &sec_ref) {
                        return Some(index);
                    }
                }
            }
            None
        }

        /// Copies the actual data contents of this `LoadedSection` to the given `destination_section`. 
        /// The following conditions must be met:    
        /// * The two sections must be from different crates (different parent crates),
        /// * The two sections must have the same size,
        /// * The given `destination_section` must be mapped as writable,
        ///   basically, it must be a .data or .bss section.
        pub (crate) fn copy_section_data_to(&self, destination_section: &mut LoadedSection) -> Result<(), &'static str> {

            let mut dest_sec_mapped_pages = destination_section.mapped_pages.lock();
            let dest_sec_data: &mut [u8] = dest_sec_mapped_pages.as_slice_mut(destination_section.mapped_pages_offset, destination_section.size)?;

            let source_sec_mapped_pages = self.mapped_pages.lock();
            let source_sec_data: &[u8] = source_sec_mapped_pages.as_slice(self.mapped_pages_offset, self.size)?;

            if dest_sec_data.len() == source_sec_data.len() {
                dest_sec_data.copy_from_slice(source_sec_data);
                // debug!("Copied data from source section {:?} {:?} ({:#X}) to dest section {:?} {:?} ({:#X})",
                //     self.typ, self.name, self.size, destination_section.typ, destination_section.name, destination_section.size);
                Ok(())
            }
            else {
                error!("This source section {:?}'s size ({:#X}) is different from the destination section {:?}'s size ({:#X})",
                    self.name, self.size, destination_section.name, destination_section.size);
                Err("this source section has a different length than the destination section")
            }
        }
    }

    impl fmt::Debug for LoadedSection {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "LoadedSection(name: {:?})", self.name)
        }
    }

    impl Drop for LoadedSection {
        fn drop(&mut self) {
            trace!("### Dropped LoadedSection {:?} {:?}", self.typ, self.name);
        }
    }

}
pub mod dependency {
    //! Structures for representing dependencies between sections.
    //! 
    //! Dependencies work as follows:
    //!  
    //! If one section `A` references or uses another section `B`, 
    //! then we colloquially say that *`A` depends on `B`*. 
    //! 
    //! In this scenario, `A` has a `StrongDependency` on `B`,
    //! and `B` has a `WeakDependent` pointing back to `A`. 
    //! 
    //! Assuming `A` and `B` are both `LoadedSection` objects,
    //! then `B.sections_i_depend_on` includes a `StrongDependency(A)`
    //! and `A.sections_dependent_on_me` includes a `WeakDependent(B)`.
    //!  
    //! In this way, the dependency graphs are fully associative,
    //! allowing a given `LoadedSection` to easily find 
    //! both its dependencies and its dependents instantly.
    //! 
    //! More importantly, it allows `A` to be dropped before `B`, 
    //! but not the other way around. 
    //! This correctly avoids dependency violations by ensuring that a section `B`
    //! is never dropped while any other section `A` relies on it.
    //! 
    //! When swapping crates, the `WeakDependent`s are actually more useful. 
    //! For example, if we want to swap the crate that contains section `B1` with a new one `B2`, 
    //! then we can immediately find all of the section `A`s that depend on `B1` 
    //! by iterating over `B1.sections_dependent_on_me`. 
    //! To complete the swap and fully replace `B1` with `B2`, 
    //! we would do the following (pseudocode):
    //! ```
    //! for secA in B1.sections_dependent_on_me {     
    //!     change secA's relocation to point to B1     
    //!     add WeakDependent(secA) to B2.sections_dependent_on_me     
    //!     remove StrongDependency(B1) from secA.sections_i_depend_on     
    //!     add StrongDependency(B2) to secA.sections_i_depend_on      
    //!     remove WeakDependent(secA) from B1.sections_dependent_on_me (current iterator)     
    //! }
    //! ```
    //! 

    use xmas_elf;
    use metadata::{StrongSectionRef, WeakSectionRef};
    use goblin::elf::reloc::*;

    /// A representation that the owner `A` of (a `LoadedSection` object containing) this struct
    /// depends on the given `section` `B` in this struct.
    /// The dependent section `A` is not specifically included here;
    /// since it's the owner of this struct, it's implicit that it's the dependent one.
    ///  
    /// A dependency is a strong reference to another `LoadedSection` `B`,
    /// because that other section `B` shouldn't be removed as long as there are still sections (`A`) that depend on it.
    /// 
    /// This is the inverse of the [`WeakDependency`](#struct.WeakDependency) type.
    #[derive(Debug, Clone)]
    pub struct StrongDependency {
        /// A strong reference to the `LoadedSection` `B` that the owner of this struct (`A`) depends on.
        pub section: StrongSectionRef,
        /// The details of the relocation action that was performed.
        pub relocation: RelocationEntry,
    }


    /// A representation that the `section` `A` in this struct
    /// depends on the owner `B` of (the `LoadedSection` object containing) this struct. 
    /// The target dependency `B` is not specifically included here; 
    /// it's implicitly the owner of this struct.
    ///  
    /// This is a weak reference to another `LoadedSection` `A`,
    /// because it is okay to remove a section `A` that depends on the owning section `B` before removing `B`.
    /// Otherwise, there would be an infinitely recursive dependency, and neither `A` nor `B` could ever be dropped.
    /// This design allows for `A` to be dropped before `B`, because there is no dependency ordering violation there.
    /// 
    /// This is the inverse of the [`StrongDependency`](#struct.StrongDependency) type.
    #[derive(Debug, Clone)]
    pub struct WeakDependent {
        /// A weak reference to the `LoadedSection` `A` that depends on the owner `B` of this struct.
        pub section: WeakSectionRef,
        /// The details of the relocation action that was performed.
        pub relocation: RelocationEntry,
    }


    /// The information necessary to calculate and write a relocation value,
    /// based on a source section and a target section, in which a value 
    /// based on the location of the source section is written somwhere in the target section.
    #[derive(Copy, Clone, Eq, PartialEq)]
    pub struct RelocationEntry {
        /// The type of relocation calculation that is performed 
        /// to connect the target section to the source section.
        pub typ: u32,
        /// The value that is added to the source section's address 
        /// when performing the calculation of the source value that is written to the target section.
        pub addend: usize,
        /// The offset from the starting virtual address of the target section
        /// that specifies where the relocation value should be written.
        pub offset: usize,
    }

    use core::fmt;
    impl fmt::Debug for RelocationEntry {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "Relocation Entry {{ typ: {:#X}, addend: {:#X}, offset: {:#X}}}",
                    self.typ, self.addend, self.offset
                )
            }
    }

    impl RelocationEntry {
        pub fn from_elf_relocation(rela_entry: &xmas_elf::sections::Rela<u64>) -> RelocationEntry {
            RelocationEntry {
                typ: rela_entry.get_type(),
                addend: rela_entry.get_addend() as usize,
                offset: rela_entry.get_offset() as usize,
            }
        }

        /// Returns true if the relocation type results in a relocation calculation
        /// in which the source value written into the target section 
        /// does NOT depend on the target section's address itself in any way 
        /// (i.e., it only depends on the source section)
        pub fn is_absolute(&self) -> bool {
            match self.typ {
                R_X86_64_32 | 
                R_X86_64_64 => true,
                _ => false,
            }
        }
    }



    /// A representation that the section that owns this struct 
    /// has a dependency on the given `source_sec`, *in the same crate*.
    /// The dependency itself is specified via the other section's shndx.
    #[derive(Debug, Clone)]
    pub struct InternalDependency {
        pub relocation: RelocationEntry,
        pub source_sec_shndx: usize,
    }
    impl InternalDependency {
        pub fn new(relocation: RelocationEntry, source_sec_shndx: usize) -> InternalDependency {
            InternalDependency {
                relocation, source_sec_shndx
            }
        }
    }
}

use self::metadata::*;
use self::dependency::*;


lazy_static! {
    /// The initial `CrateNamespace` that all crates are added to by default,
    /// unless otherwise specified for crate swapping purposes.
    static ref DEFAULT_CRATE_NAMESPACE: CrateNamespace = CrateNamespace::with_name("default");
}

pub fn get_default_namespace() -> &'static CrateNamespace {
    &DEFAULT_CRATE_NAMESPACE
}

/// This should be a const, but Rust doesn't like OR-ing bitflags as a const expression.
#[allow(non_snake_case)]
pub fn TEXT_SECTION_FLAGS() -> EntryFlags {
    EntryFlags::PRESENT
}
/// This should be a const, but Rust doesn't like OR-ing bitflags as a const expression.
#[allow(non_snake_case)]
pub fn RODATA_SECTION_FLAGS() -> EntryFlags {
    EntryFlags::PRESENT | EntryFlags::NO_EXECUTE
}
/// This should be a const, but Rust doesn't like OR-ing bitflags as a const expression.
#[allow(non_snake_case)]
pub fn DATA_BSS_SECTION_FLAGS() -> EntryFlags {
    EntryFlags::PRESENT | EntryFlags::NO_EXECUTE | EntryFlags::WRITABLE
}


/// A list of one or more `SwapRequest`s that is used by the `swap_crates` function.
pub type SwapRequestList = Vec<SwapRequest>;
  

/// This struct is used to specify the details of a crate-swapping operation,
/// in which an "old" crate is removed and replaced with a "new" crate
/// that is then used in place of that old crate. 
/// 
/// # Important Note
/// When swapping out an old crate, the new crate must satisfy all of the dependencies
/// that other crates had on that old crate. 
/// However, this function will completely remove the old crate and its symbols from that `CrateNamespace`,
/// so it may be useful to re-expose or "mirror" the new crate's sections as symbols with names 
/// that match the relevant sections from the old crate.
/// This satisfies other crates' dependenices on the old crate while allowing the new crate to exist normally,
/// which means that the new crate's symbols appear both as those from the new crate itself 
/// in addition to those from the old crate that was replaced. 
/// To do this, set the `reexport_new_symbols_as_old` option to `true`.
/// 
/// However, **enabling this option is potentially dangerous**, as you must take responsibility 
/// for ensuring that the new crate can safely and correctly replace the old crate, 
/// and that using the new crate in place of the old crate will not break any other dependent crates.
/// This will necessarily ignore the hashes when matching symbols across the old and new crate, 
/// and hashes are how Theseus ensures that different crates and functions are linked to the correct version of each other.
/// However, this does make the swapping operation much simpler to use, 
/// because you will likely need to swap far fewer crates at once, 
/// and as a convenience bonus, you don't have to specify the exact versions (hashes) of each crate.
/// 
/// Example: the function `drivers::init::h...` in the `drivers` crate depends 
/// on the function `keyboard::init::hABC` in the `keyboard` crate.
/// We call `swap_crates()` to replace the `keyboard` crate with a new crate called `keyboard_new`.
/// The new `keyboard_new` crate has a function called `keyboard_new::init::hDEF`, 
/// which now appears in the symbols map twice: 
/// * one symbol `keyboard_new::init::hDEF`, which is the normal behavior when loading a crate, and
/// * one symbol `keyboard::init::hABC`, which exactly matches the function from the old `keyboard` crate.
/// In this way, the single function in the new crate `keyboard_new` appears twice in the symbol map
/// under different names, allowing it to fulfill dependencies on both the old crate and the new crate.
/// 
#[derive(Debug, Eq, PartialEq, Hash)]
pub struct SwapRequest {
    /// The name of the old crate that will be replaced by the new crate.
    /// This will be used to search the `CrateNamespace` to find an existing `LoadedCrate`.
    old_crate_name: String,
    /// The `ModuleArea` containing the object file for the new crate that will replace the old crate. 
    new_crate_module_area: &'static ModuleArea,
    /// Whether to expose the new crate's sections with symbol names that match those from the old crate.
    /// For more details, see the above docs for this struct.
    reexport_new_symbols_as_old: bool,
}
impl SwapRequest {
    /// Create a new `SwapRequest` that, when given to `swap_crates()`, 
    /// will swap out the given old crate and replace it with the given new crate,
    /// and optionally re-export the new crate's symbols 
    pub fn new(
        old_crate_name: String, 
        new_crate_module_area: &'static ModuleArea, 
        reexport_new_symbols_as_old: bool,
    ) -> SwapRequest {
        SwapRequest {
            old_crate_name, new_crate_module_area, reexport_new_symbols_as_old,
        }
    }
}


/// A "symbol map" from a fully-qualified demangled symbol String  
/// to weak reference to a `LoadedSection`.
/// This is used for relocations, and for looking up function names.
pub type SymbolMap = Trie<BString, WeakSectionRef>;
pub type SymbolMapIter<'a> = qp_trie::Iter<'a, &'a BString, &'a WeakSectionRef>;


/// This struct represents a namespace of crates and their "global" (publicly-visible) symbols.
/// A crate namespace struct is basically a container around many crates 
/// that have all been loaded and linked against each other, 
/// completely separate and in isolation from any other crate namespace 
/// (although a given crate may be present in multiple namespaces). 
pub struct CrateNamespace {
    /// An identifier for this namespace, just for convenience.
    name: String,

    /// The list of all the crates in this namespace,
    /// stored as a map in which the crate's String name
    /// is the key that maps to the value, a strong reference to a crate.
    /// It is a strong reference because a crate must not be removed
    /// as long as it is part of any namespace,
    /// and a single crate can be part of multiple namespaces at once.
    /// For example, the "core" (Rust core library) crate is essentially
    /// part of every single namespace, simply because most other crates rely upon it. 
    pub crate_tree: Mutex<Trie<BString, StrongCrateRef>>,

    /// The "system map" of all global (publicly-visible) symbols
    /// that are present in all of the crates in this `CrateNamespace`.
    /// Maps a fully-qualified symbol name string to a corresponding `LoadedSection`,
    /// which is guaranteed to be part of one of the crates in this `CrateNamespace`.  
    /// Symbols declared as "no_mangle" will appear in the map with no crate prefix, as expected.
    symbol_map: Mutex<SymbolMap>,

    /// The set of crates that have been previously unloaded (e.g., swapped out) from this namespace. 
    /// These are kept in memory as a performance optimization, such that if 
    /// they are ever requested to be swapped in again, we can swap them back in 
    /// almost instantly by avoiding the  expensive procedure of re-loading them into memory.
    /// 
    /// The unloaded cached crates are stored in the form of a CrateNamespace itself,
    /// such that we can easily query the cache for crates and symbols by name.
    /// 
    /// This is soft state that can be removed at any time with no effect on correctness.
    unloaded_crate_cache: Mutex<HashMap<SwapRequestList, CrateNamespace>>,
}

impl CrateNamespace {
    /// Creates a new `CrateNamespace` that is completely empty. 
    pub fn new() -> CrateNamespace {
        CrateNamespace::with_name("")
    } 


    /// Creates a new `CrateNamespace` that is completely empty, and is given the specified `name`.
    pub fn with_name(name: &str) -> CrateNamespace {
        CrateNamespace {
            name: String::from(name),
            crate_tree: Mutex::new(Trie::new()),
            symbol_map: Mutex::new(SymbolMap::new()),
            unloaded_crate_cache: Mutex::new(HashMap::new()),
        }
    } 


    /// Returns a list of all of the crate names currently loaded into this `CrateNamespace`.
    /// This is a slow method mostly for debugging, since it allocates new Strings for each crate name.
    pub fn crate_names(&self) -> Vec<String> {
        self.crate_tree.lock().keys().map(|bstring| String::from(bstring.as_str())).collect()
    }


    /// Acquires the lock on this `CrateNamespace`'s crate list and looks for the crate 
    /// that matches the given `crate_name`, if it exists in this namespace.
    /// 
    /// Returns a `StrongCrateReference` that **has not** been marked as a shared crate reference,
    /// so if the caller wants to keep the returned `StrongCrateRef` as a shared crate 
    /// that jointly exists in another namespace, they should invoke the 
    /// [`CowArc::share()`](cow_arc/CowArc.share.html) function on the returned value.
    pub fn get_crate(&self, crate_name: &str) -> Option<StrongCrateRef> {
        self.crate_tree.lock().get_str(crate_name).map(|r| CowArc::clone_shallow(r))
    }

    /// Returns a reference to the `LoadedCrate` that corresponds to the given crate_name_prefix,
    /// *if and only if* the list of `LoadedCrate`s only contains a single possible match.
    /// 
    /// # Important Usage Note
    /// To avoid greedily matching more crates than expected, you may wish to end the `crate_name_prefix` with "`-`".
    /// This may provide results more in line with the caller's expectations; see the last example below about a trailing "`-`". 
    /// This works because the delimiter between a crate name and its trailing hash value is "`-`".
    /// 
    /// # Example
    /// * This `CrateNamespace` contains the crates `my_crate-843a613894da0c24` and 
    ///   `my_crate_new-933a635894ce0f12`. 
    ///   Calling `get_crate_starting_with("my_crate::foo")` will return None,
    ///   because it will match both `my_crate` and `my_crate_new`. 
    ///   To match only `my_crate`, call this function as `get_crate_starting_with("my_crate-")`
    ///   (note the trailing "`-`").
    pub fn get_crate_starting_with(&self, crate_name_prefix: &str) -> Option<StrongCrateRef> { 
        let crates = self.crate_tree.lock();
        let mut iter = crates.iter_prefix_str(crate_name_prefix);
        iter.next()
            .filter(|_| iter.next().is_none()) // ensure single element
            .map(|(_key, val)| val.clone())
    }


    /// Loads the specified application crate into memory, allowing it to be invoked.
    /// 
    /// The argument `load_symbols_as_singleton` determines what is done with the newly-loaded crate:      
    /// * If true, this application is loaded as a system-wide singleton crate and added to this namespace, 
    ///   and its public symbols are added to this namespace's symbol map, allowing other applications to depend upon it.
    /// * If false, this application is loaded normally and not added to this namespace in any way,
    ///   allowing it to be loaded again in the future as a totally separate instance.
    /// 
    /// Returns a Result containing the new crate itself.
    pub fn load_application_crate(
        &self, 
        crate_module: &'static ModuleArea, 
        kernel_mmi: &mut MemoryManagementInfo, 
        load_symbols_as_singleton: bool,
        verbose_log: bool
    ) -> Result<StrongCrateRef, &'static str> {
        let (crate_type, crate_name) = CrateType::from_module_name(crate_module.name())?;
        if crate_type != CrateType::Application {
            error!("load_application_crate() cannot be used for crate \"{}\", only for application crate modules starting with \"{}\"",
                crate_module.name(), CrateType::Application.prefix());
            return Err("load_application_crate() can only be used for application crate modules");
        }
        
        debug!("load_application_crate: trying to load \"{}\" application module", crate_module.name());
        let temp_module_mapping = map_crate_module(crate_module, kernel_mmi)?;
        let (new_crate_ref, elf_file) = self.load_crate_sections(&temp_module_mapping, crate_module, crate_module.size(), crate_name, kernel_mmi, verbose_log)?;
        
        // no backup namespace when loading applications, they must be able to find all symbols in only this namespace (&self)
        self.perform_relocations(&elf_file, &new_crate_ref, None, kernel_mmi, verbose_log)?;

        if load_symbols_as_singleton {
            // if this is a singleton application, we add its public symbols (except "main")
            let new_crate = new_crate_ref.lock_as_ref();
            self.add_symbols_filtered(new_crate.sections.values(), 
                |sec| sec.name != "main", 
                verbose_log
            );
            self.crate_tree.lock().insert_str(&new_crate.crate_name, CowArc::clone_shallow(&new_crate_ref));
        } else {
            let new_crate = new_crate_ref.lock_as_ref();
            info!("loaded new application crate module: {}, num sections: {}", new_crate.crate_name, new_crate.sections.len());
        }
        Ok(new_crate_ref)

        // temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
    }


    /// Loads the specified kernel crate into memory, allowing it to be invoked.  
    /// Returns a Result containing the number of symbols that were added to the symbol map
    /// as a result of loading this crate.
    /// # Arguments
    /// * `crate_module`: the crate that should be loaded into this `CrateNamespace`.
    /// * `backup_namespace`: the `CrateNamespace` that should be searched for missing symbols 
    ///   (for relocations) if a symbol cannot be found in this `CrateNamespace`. 
    ///   For example, the default namespace could be used by passing in `Some(get_default_namespace())`.
    ///   If `backup_namespace` is `None`, then no other namespace will be searched, 
    ///   and any missing symbols will return an `Err`. 
    /// * `kernel_mmi`: a mutable reference to the kernel's `MemoryManagementInfo`.
    /// * `verbose_log`: a boolean value whether to enable verbose_log logging of crate loading actions.
    pub fn load_kernel_crate(
        &self,
        crate_module: &'static ModuleArea, 
        backup_namespace: Option<&CrateNamespace>, 
        kernel_mmi: &mut MemoryManagementInfo, 
        verbose_log: bool
    ) -> Result<usize, &'static str> {

        let (crate_type, crate_name) = CrateType::from_module_name(crate_module.name())?;
        if crate_type != CrateType::Kernel {
            error!("load_kernel_crate() cannot be used for crate \"{}\", only for kernel crate modules starting with \"{}\"",
                crate_module.name(), CrateType::Kernel.prefix());
            return Err("load_kernel_crate() can only be used for kernel crate modules");
        }

        debug!("load_kernel_crate: trying to load \"{}\" kernel crate", crate_name);
        let temp_module_mapping = map_crate_module(crate_module, kernel_mmi)?;
        let (new_crate_ref, elf_file) = self.load_crate_sections(&temp_module_mapping, crate_module, crate_module.size(), crate_name, kernel_mmi, verbose_log)?;
        self.perform_relocations(&elf_file, &new_crate_ref, backup_namespace, kernel_mmi, verbose_log)?;
        let (new_crate_name, new_syms) = {
            let new_crate = new_crate_ref.lock_as_ref();
            let new_syms = self.add_symbols(new_crate.sections.values(), verbose_log);
            (new_crate.crate_name.clone(), new_syms)
        };
            
        info!("loaded module {:?} as new crate {:?}, {} new symbols.", crate_module.name(), new_crate_name, new_syms);
        self.crate_tree.lock().insert(new_crate_name.into(), new_crate_ref);
        Ok(new_syms)
        
        // plc.temp_module_mapping is automatically unmapped when it falls out of scope here (frame allocator must not be locked)
    }

    
    /// This function first loads all of the given crates' sections and adds them to the symbol map,
    /// and only after *all* crates are loaded does it move on to linking/relocation calculations. 
    /// 
    /// This allows multiple object files with circular dependencies on one another
    /// to be loaded "atomically", i.e., as a single unit. 
    /// 
    /// # Example
    /// If crate `A` depends on crate `B`, and crate `B` depends on crate `A`,
    /// this function will load both crate `A` and `B` before trying to resolve their dependencies individually. 
    pub fn load_kernel_crates<I>(
        &self,
        new_modules: I,
        backup_namespace: Option<&CrateNamespace>,
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool,
    ) -> Result<(), &'static str> 
        where I: Iterator<Item = &'static ModuleArea> + Clone
    {
        // first we map all of the crates' ModuleAreas
        let mappings = {
            let mut mappings: Vec<MappedPages> = Vec::new(); //Vec::with_capacity(len);
            for crate_module in new_modules.clone() {
                debug!("mapping crate_module {:?}", crate_module);
                mappings.push(map_crate_module(crate_module, kernel_mmi)?);
            }
            mappings
        };

        let mut partially_loaded_crates: Vec<(StrongCrateRef, ElfFile)> = Vec::with_capacity(mappings.len()); 

        // then we do all of the section parsing and loading
        for (i, crate_module) in new_modules.enumerate() {
            let temp_module_mapping = mappings.get(i).ok_or("BUG: mapped crate module successfully but couldn't retrieve mapping (WTF?)")?;
            let (new_crate, elf_file) = self.load_crate_sections(
                temp_module_mapping, 
                crate_module,
                crate_module.size(),
                CrateType::from_module_name(crate_module.name())?.1,
                kernel_mmi, 
                verbose_log
            )?;
            let _new_syms = self.add_symbols(new_crate.lock_as_ref().sections.values(), verbose_log);
            partially_loaded_crates.push((new_crate, elf_file));
        }
        
        // then we do all of the relocations 
        for (new_crate_ref, elf_file) in partially_loaded_crates {
            self.perform_relocations(&elf_file, &new_crate_ref, backup_namespace, kernel_mmi, verbose_log)?;
            let name = new_crate_ref.lock_as_ref().crate_name.clone();
            self.crate_tree.lock().insert(name.into(), new_crate_ref);
        }

        Ok(())
    }




    /// Duplicates this `CrateNamespace` into a new `CrateNamespace`, 
    /// but uses a copy-on-write/clone-on-write semantic that creates 
    /// a special shared reference to each crate that indicates it is shared across multiple namespaces.
    /// 
    /// In other words, crates in the new namespace returned by this fucntions 
    /// are fully shared with crates in *this* namespace, 
    /// until either namespace attempts to modify a shared crate in the future.
    /// 
    /// When modifying crates in the new namespace, such as with the funtions 
    /// [`swap_crates`](#method.swap_crates), any crates in the new namespace
    /// that are still shared with the old namespace will be deeply copied into a new crate,
    /// and then that new crate will be modified in whichever way specified. 
    /// For example, if you swapped one crate `A` in the new namespace returned from this function
    /// and loaded a new crate `A2` in its place,
    /// and two other crates `B` and `C` depended on that newly swapped-out `A`,
    /// then `B` and `C` would be transparently deep copied before modifying them to depend on
    /// the new crate `A2`, and you would be left with `B2` and `C2` as deep copies of `B` and `C`,
    /// that now depend on `A2` instead of `A`. 
    /// The existing versions of `B` and `C` would still depend on `A`, 
    /// but they would no longer be part of the new namespace. 
    /// 
    pub fn clone_on_write(&self) -> CrateNamespace {
        let new_crate_tree = self.crate_tree.lock().clone();
        let new_symbol_map = self.symbol_map.lock().clone();

        CrateNamespace {
            name: self.name.clone(),
            crate_tree: Mutex::new(new_crate_tree),
            symbol_map: Mutex::new(new_symbol_map),
            unloaded_crate_cache: Mutex::new(HashMap::new()),
        }
    }


    /// Swaps in new modules to replace existing crates this in `CrateNamespace`.
    /// 
    /// See the documentation of the [`SwapRequest`](#struct.SwapRequest.html) struct for more details.
    /// 
    /// In general, the strategy for replacing an old module `C` with a new module `C2` consists of three simple steps:
    /// 1) Load the new replacement module `C2`.
    /// 2) Set up new relocation entries that redirect all module's dependencies on the old module `C` to the new module `C2`.
    /// 3) Remove module `C` and clean it up, e.g., removing its entries from the symbol map.
    ///    Save the removed crate (and its symbol subtrie) in a cache for later use to expedite future swapping operations.
    /// 
    /// This `CrateNamespace` (self) is used as the backup namespace for resolving unknown symbols.
    /// 
    /// Upon a successful return, this namespace (self) will have the new crates in place of the old ones,
    /// and the old crates will be completely removed from this namespace. 
    /// 
    /// # Arguments
    /// * `swap_requests`: a list of several `SwapRequest`s, in order to allow swapping multiple crates all at once 
    ///   as a single "atomic" procedure, which prevents weird linking/relocation errors, 
    ///   such as a new crate linking against an old crate that already exists in this namespace
    ///   instead of linking against the new one that we want to replace that old crate with. 
    /// * `kernel_mmi`: a mutable reference to the kernel's `MemoryManagementInfo`.
    /// * `verbose_log`: enable verbose logging.
    /// 
    /// # Note
    /// This function currently makes no attempt to guarantee correct operation after a crate is swapped. 
    /// For example, if the new crate changes a function or data structure, there is no guarantee that 
    /// other crates will be swapped alongside that one -- that responsibility is currently left to the caller.
    pub fn swap_crates(
        &self,
        swap_requests: SwapRequestList,
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool,
    ) -> Result<(), &'static str> {

        // First, before we perform any expensive crate loading, let's try an optimization
        // based on cached crates that were unloaded during a previous swap operation. 
        let (namespace_of_new_crates, is_optimized) = if let Some(cached_crates) = self.unloaded_crate_cache.lock().remove(&swap_requests) {
            // info!("Using optimized swap routine to swap in cached crates: {:?}", cached_crates.crate_names());
            (cached_crates, true)
        } else {
            // If no optimization is possible (no cached crates exist for this swap request), 
            // then create a new CrateNamespace and load all of the new crate modules into it from scratch.
            let nn = CrateNamespace::with_name(&format!("temp_swap--{:?}", swap_requests));
            let module_iter = swap_requests.iter().map(|swap_req| swap_req.new_crate_module_area);
            nn.load_kernel_crates(module_iter, Some(self), kernel_mmi, verbose_log)?;
            (nn, false)
        };

        let mut future_swap_requests: SwapRequestList = SwapRequestList::with_capacity(swap_requests.len());
        let cached_crates: CrateNamespace = CrateNamespace::new();

        // Now that we have loaded all of the new modules into the new namepsace in isolation,
        // we simply need to remove all of the old crates
        // and fix up all of the relocations `WeakDependents` for each of the existing sections
        // that depend on the old crate that we're replacing here,
        // such that they refer to the new_module instead of the old_crate.
        for req in swap_requests {
            let SwapRequest { old_crate_name, new_crate_module_area, reexport_new_symbols_as_old } = req;
            let (_new_crate_type, new_crate_name) = CrateType::from_module_name(new_crate_module_area.name())?;
            if self.get_crate(new_crate_name).is_some() {
                error!("swap_crates(): the requested new crate {:?} was already loaded into this namespace!", new_crate_name);
                return Err("swap_crates(): the requested new crate was already loaded into this namespace!");
            }
            
            let old_crate_ref = self.get_crate(&old_crate_name).ok_or_else(|| {
                error!("swap_crates(): couldn't find requested old_crate {:?}", old_crate_name);
                "swap_crates(): couldn't find requested old crate"
            })?;
            let old_crate = old_crate_ref.lock_as_mut().ok_or_else(|| {
                error!("TODO FIXME: swap_crates(), old_crate: {:?}, doesn't yet support deep copying shared crates to get a new exlusive mutable instance", old_crate_ref);
                "TODO FIXME: swap_crates() doesn't yet support deep copying shared crates to get a new exlusive mutable instance"
            })?;

            let new_crate_ref = if is_optimized {
                // debug!("swap_crates(): OPTIMIZED: trying to get new crate {:?} from cache", new_crate_name);
                namespace_of_new_crates.get_crate(new_crate_name)
                    .ok_or_else(|| "BUG: swap_crates(): Couldn't get new crate from optimized cache")?
            } else {
                // debug!("trying to get newly-loaded crate {:?} from temp namespace", new_crate_name);
                namespace_of_new_crates.get_crate(new_crate_name)
                    .ok_or_else(|| "BUG: Couldn't get new crate that should've just been loaded into a new temporary namespace")?
            };


            // scope the lock for `self.symbol_map` and `new_crate_ref`
            {
                let mut this_symbol_map = self.symbol_map.lock();
                let mut new_crate = new_crate_ref.lock_as_mut()
                    .ok_or_else(|| "BUG: swap_crates(): new_crate was unexpectedly shared in another namespace (couldn't get as exclusively mutable)...?")?;
                
                // currently we're always clearing out the new crate's reexports because we recalculate them every time
                new_crate.reexported_symbols.clear();

                let old_crate_name_without_hash = String::from(old_crate.crate_name_without_hash());
                let new_crate_name_without_hash = String::from(new_crate.crate_name_without_hash());
                let crates_have_same_name = old_crate_name_without_hash == new_crate_name_without_hash;

                // We need to find all of the weak dependents (sections that depend on sections in the old crate that we're removing)
                // and replace them by rewriting their relocation entries to point to that section in the new_crate.
                // We also use this loop to remove all of the old_crate's symbols from this namespace's symbol map.
                // 
                // Note that we only need to iterate through sections from the old crate that are public/global,
                // i.e., those that were previously added to this namespace's symbol map,
                // because other crates could not possibly depend on non-public sections in the old crate.
                for old_sec_name in &old_crate.global_symbols {
                    let old_sec_ref = this_symbol_map.get(old_sec_name)
                        .and_then(|weak_sec_ref| weak_sec_ref.upgrade())
                        .ok_or("BUG: swap_crates(): couldn't get/upgrade old crate's section")?;
                    // debug!("swap_crates(): old_sec_name: {:?}, old_sec: {:?}", old_sec_name, old_sec_ref);
                    let mut old_sec = old_sec_ref.lock();
                    let old_sec_name_without_hash = old_sec.name_without_hash();


                    // This closure finds the section in the `new_crate` that corresponds to the given `old_sec` from the `old_crate`.
                    // And, if enabled, it will reexport that new section under the same name as the `old_sec`.
                    // We put this procedure in a closure because it's relatively expensive, allowing us to run it only when necessary.
                    let mut find_corresponding_new_section = |new_crate_reexported_symbols: &mut BTreeSet<BString>| {
                        // Use the new namespace to find the new source_sec that old target_sec should point to.
                        // The new source_sec must have the same name as the old one (old_sec here),
                        // otherwise it wouldn't be a valid swap -- the target_sec's parent crate should have also been swapped.
                        // The new namespace should already have that symbol available (i.e., we shouldn't have to load it on demand);
                        // if not, the swapping action was never going to work and we shouldn't go through with it.

                        // Find the section in the new crate that "fuzzily" matches the current section from the old crate. There should only be one possible match.
                        let new_crate_source_sec = if crates_have_same_name {
                            namespace_of_new_crates.get_symbol_starting_with(old_sec_name_without_hash)
                        } else {
                            if let Some(s) = replace_containing_crate_name(old_sec_name_without_hash, &old_crate_name_without_hash, &new_crate_name_without_hash) {
                                namespace_of_new_crates.get_symbol_starting_with(&s)
                            } else {
                                namespace_of_new_crates.get_symbol_starting_with(old_sec_name_without_hash)
                            }
                        }.upgrade().ok_or_else(|| {
                            error!("swap_crates(): couldn't find section in the new crate that corresponds to a fuzzy match of the old section {:?}", old_sec.name);
                            "couldn't find section in the new crate that corresponds to a fuzzy match of the old section"
                        })?;
                        // debug!("swap_crates(): found fuzzy match for old source_sec {:?} in new crate: {:?}", old_sec.name, new_crate_source_sec);
                        if reexport_new_symbols_as_old && old_sec.global {
                            // reexport the new source section under the old sec's name, i.e., redirect the old mapping to the new source sec
                            let reexported_name = BString::from(old_sec.name.as_str());
                            new_crate_reexported_symbols.insert(reexported_name.clone());
                            let _old_val = this_symbol_map.insert(reexported_name, Arc::downgrade(&new_crate_source_sec));
                            if _old_val.is_none() { 
                                warn!("swap_crates(): reexported new crate section that replaces old section {:?}, but that old section unexpectedly didn't exist in the symbol map", old_sec.name);
                            }
                        }
                        Ok(new_crate_source_sec)

                        // We aren't using exact matches right now, since that basically never happens across 2 different crates, even with the same crate name
                        // else {
                        //     namespace_of_new_crates.get_symbol(&old_sec.name)
                        //         .upgrade()
                        //         .ok_or_else(|| {
                        //             error!("swap_crates(): couldn't find section in the new crate that corresponds to an exact match of the old section {:?}", old_sec.name);
                        //             "couldn't find section in the new crate that corresponds to an exact match of the old section"
                        //         })
                        // }
                    };


                    // the section from the `new_crate` that corresponds to the `old_sec_ref` from the `old_crate`
                    let mut new_sec_ref: Option<StrongSectionRef> = None;

                    for weak_dep in &old_sec.sections_dependent_on_me {
                        let target_sec_ref = weak_dep.section.upgrade().ok_or_else(|| "couldn't upgrade WeakDependent.section")?;
                        let mut target_sec = target_sec_ref.lock();
                        let relocation_entry = weak_dep.relocation;

                        // get the section from the new crate that corresponds to the `old_sec`
                        let new_source_sec_ref = if let Some(ref nsr) = new_sec_ref {
                            // trace!("using cached version of new source section");
                            nsr
                        } else {
                            // trace!("calculating new source section from scratch");
                            let nsr = find_corresponding_new_section(&mut new_crate.reexported_symbols)?;
                            new_sec_ref.get_or_insert(nsr)
                        };
                        let mut new_source_sec = new_source_sec_ref.lock();
                        // debug!("swap_crates(): target_sec: {:?}, old source sec: {:?}, new source sec: {:?}", target_sec.name, old_sec.name, new_source_sec.name);

                        // If the target_sec's mapped pages aren't writable (which is common in the case of swapping),
                        // then we need to temporarily remap them as writable here so we can fix up the target_sec's new relocation entry.
                        {
                            let mut target_sec_mapped_pages = target_sec.mapped_pages.lock();
                            let target_sec_initial_flags = target_sec_mapped_pages.flags();
                            if !target_sec_initial_flags.is_writable() {
                                if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                                    target_sec_mapped_pages.remap(active_table, target_sec_initial_flags | EntryFlags::WRITABLE)?;
                                }
                                else {
                                    return Err("couldn't get kernel's active page table");
                                }
                            }

                            write_relocation(
                                relocation_entry, 
                                &mut target_sec_mapped_pages, 
                                target_sec.mapped_pages_offset, 
                                new_source_sec.virt_addr(), 
                                verbose_log
                            )?;

                            // If we temporarily remapped the target_sec's mapped pages as writable, undo that here
                            if !target_sec_initial_flags.is_writable() {
                                if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                                    target_sec_mapped_pages.remap(active_table, target_sec_initial_flags)?;
                                }
                                else {
                                    return Err("couldn't get kernel's active page table");
                                }
                            };
                        }
                        
                        // Tell the new source_sec that the existing target_sec depends on it.
                        // Note that we don't need to do this if we're re-swapping in a cached crate,
                        // because that crate's sections' dependents are already properly set up from when it was first swapped in.
                        if !is_optimized {
                            new_source_sec.sections_dependent_on_me.push(WeakDependent {
                                section: Arc::downgrade(&target_sec_ref),
                                relocation: relocation_entry,
                            });
                        }

                        // Tell the existing target_sec that it no longer depends on the old source section (old_sec_ref),
                        // and that it now depends on the new source_sec.
                        let mut found_strong_dependency = false;
                        for mut strong_dep in target_sec.sections_i_depend_on.iter_mut() {
                            if Arc::ptr_eq(&strong_dep.section, &old_sec_ref) && strong_dep.relocation == relocation_entry {
                                strong_dep.section = Arc::clone(&new_source_sec_ref);
                                found_strong_dependency = true;
                                break;
                            }
                        }
                        if !found_strong_dependency {
                            error!("Couldn't find/remove the existing StrongDependency from target_sec {:?} to old_sec {:?}",
                                target_sec.name, old_sec.name);
                            return Err("Couldn't find/remove the target_sec's StrongDependency on the old crate section");
                        }
                    }
                }


                // Go through all the BSS sections and copy over the old_sec into the new source_sec,
                // if they represent a static variable (state spill that would otherwise result in a loss of data).
                // Currently, AFAIK, static variables (states) only exist in the form of .bss sections
                for old_sec_ref in old_crate.bss_sections.values() {
                    let old_sec = old_sec_ref.lock();
                    let old_sec_name_without_hash = old_sec.name_without_hash();
                    // get the section from the new crate that corresponds to the `old_sec`
                    let new_dest_sec_ref = {
                        let mut iter = if crates_have_same_name {
                            new_crate.bss_sections.iter_prefix_str(old_sec_name_without_hash)
                        } else {
                            if let Some(s) = replace_containing_crate_name(old_sec_name_without_hash, &old_crate_name_without_hash, &new_crate_name_without_hash) {
                                new_crate.bss_sections.iter_prefix_str(&s)
                            } else {
                                new_crate.bss_sections.iter_prefix_str(old_sec_name_without_hash)
                            }
                        };
                        iter.next()
                            .filter(|_| iter.next().is_none()) // ensure single element
                            .map(|(_key, val)| val)
                    }.ok_or_else(|| 
                        "couldn't find destination section in new crate for copying old_sec's data into (BSS state transfer)"
                    )?;

                    // debug!("swap_crates(): copying BSS section from old {:?} to new {:?}", &*old_sec, new_dest_sec_ref);
                    old_sec.copy_section_data_to(&mut new_dest_sec_ref.lock())?;
                }

                
                // Remove the old crate from this namespace, and remove its sections' symbols too
                if let Some(removed_old_crate) = self.crate_tree.lock().remove_str(&old_crate.crate_name) {
                    // Here, `old_crate` and `removed_old_crate` are the same, but `old_crate` is already locked, 
                    // so we use that instead of trying to lock `removed_old_crate` again, because it would cause deadlock.
                    
                    // info!("  Removed old crate {}", old_crate.crate_name);

                    // Here, we setup the crate cache to enable the removed old crate to be quickly swapped back in in the future.
                    // This removed old crate will be useful when a future swap request includes the following:
                    // (1) the future `new_crate_module_area`        ==  the current `old_crate.object_file`
                    // (2) the future `old_crate_name`               ==  the current `new_crate_name`
                    // (3) the future `reexport_new_symbols_as_old`  ==  true if the old crate had any reexported symbols
                    //     -- to understand this, see the docs for `LoadedCrate.reexported_prefix`
                    let future_swap_req = SwapRequest::new(String::from(new_crate_name), old_crate.object_file, !old_crate.reexported_symbols.is_empty());
                    future_swap_requests.push(future_swap_req);
                    
                    // Remove all of the symbols belonging to the old crate from this namespace.
                    // If reexport_new_symbols_as_old is true, we MUST NOT remove the old_crate's symbols from this symbol map,
                    // because we already replaced them above with mappings that redirect to the corresponding new crate sections.
                    if !reexport_new_symbols_as_old {
                        for symbol in &old_crate.global_symbols {
                            if this_symbol_map.remove(symbol).is_none() {
                                error!("swap_crates(): couldn't find old symbol {:?} in this namespace's symbol map!", symbol);
                                return Err("couldn't find old symbol {:?} in this namespace's symbol map!");
                            }
                        }
                    }

                    // If the old crate had reexported its symbols, we should remove those reexports here,
                    // because they're no longer active since the old crate is being removed. 
                    for sym in &old_crate.reexported_symbols {
                        this_symbol_map.remove(sym);
                    }

                    // TODO: could maybe optimize transfer of old symbols from this namespace to cached_crates namespace 
                    //       by saving the removed symbols above and directly adding them to the cached_crates.symbol_map instead of iterating over all old_crate.sections.
                    //       This wil only really be faster once qp_trie supports a non-iterator-based (non-extend) Trie merging function.
                    cached_crates.add_symbols(old_crate.sections.values(), verbose_log); 
                    cached_crates.crate_tree.lock().insert_str(&old_crate.crate_name, removed_old_crate);
                }
                else {
                    error!("BUG: swap_crates(): failed to remove old crate {}", old_crate.crate_name);
                }
            } // end of scope, drops lock for `self.symbol_map` and `new_crate_ref`

            // add the new crate and its sections' symbols to this namespace
            self.add_symbols(new_crate_ref.lock_as_ref().sections.values(), verbose_log); // TODO: later, when qp_trie supports `drain()`, we can improve this futher.
            self.crate_tree.lock().insert_str(new_crate_name, new_crate_ref);
        }

        // debug!("swap_crates() [end]: adding old_crates to cache. \n   future_swap_requests: {:?}, \n   old_crates: {:?}", 
        //     future_swap_requests, cached_crates.crate_names());
        self.unloaded_crate_cache.lock().insert(future_swap_requests, cached_crates);
        Ok(())
        // here, "namespace_of_new_crates is dropped, but its crates have already been added to the current namespace 
    }


    /// The primary internal routine for parsing and loading all of the sections.
    /// This does not perform any relocations or linking, so the crate **is not yet ready to use after this function**,
    /// since its sections are totally incomplete and non-executable.
    /// 
    /// However, it does add all of the newly-loaded crate sections to the symbol map (yes, even before relocation/linking),
    /// since we can use them to resolve missing symbols for relocations.
    /// 
    /// Parses each section in the given `ElfFile` and copies the object file contents to each section.
    /// Returns a tuple of the new `LoadedCrate`, the list of newly `LoadedSection`s, and the crate's ELF file.
    /// The list of sections is actually a map from its section index (shndx) to the `LoadedSection` itself,
    /// which is kept separate and has not yet been added to the new `LoadedCrate` beause it needs to be used for relocations.
    fn load_crate_sections<'e>(
        &self,
        mapped_pages: &'e MappedPages, 
        object_file: &'static ModuleArea,
        size_in_bytes: usize, 
        crate_name: &str, 
        kernel_mmi: &mut MemoryManagementInfo,
        _verbose_log: bool
    ) -> Result<(StrongCrateRef, ElfFile<'e>), &'static str> {
        
        // First, check to make sure this crate hasn't already been loaded. 
        // Regular, non-singleton application crates aren't added to the CrateNamespace, so they can be multiply loaded.
        if self.crate_tree.lock().contains_key_str(crate_name) {
            return Err("the crate has already been loaded, cannot load it again in the same namespace");
        }

        // Parse the given `mapped_pages` as an ELF file
        let byte_slice: &[u8] = mapped_pages.as_slice(0, size_in_bytes)?;
        let elf_file = ElfFile::new(byte_slice)?; // returns Err(&str) if ELF parse fails

        // check that elf_file is a relocatable type 
        use xmas_elf::header::Type;
        let typ = elf_file.header.pt2.type_().as_type();
        if typ != Type::Relocatable {
            error!("load_crate_sections(): crate \"{}\" was a {:?} Elf File, must be Relocatable!", crate_name, typ);
            return Err("not a relocatable elf file");
        }

        debug!("Parsing Elf kernel crate: {:?}, size {:#x}({})", crate_name, size_in_bytes, size_in_bytes);

        // allocate enough space to load the sections
        let section_pages = allocate_section_pages(&elf_file, kernel_mmi)?;
        let text_pages    = section_pages.text_pages  .map(|tp| Arc::new(Mutex::new(tp)));
        let rodata_pages  = section_pages.rodata_pages.map(|rp| Arc::new(Mutex::new(rp)));
        let data_pages    = section_pages.data_pages  .map(|dp| Arc::new(Mutex::new(dp)));

        let mut text_pages_locked   = text_pages  .as_ref().map(|tp| tp.lock());
        let mut rodata_pages_locked = rodata_pages.as_ref().map(|rp| rp.lock());
        let mut data_pages_locked   = data_pages  .as_ref().map(|dp| dp.lock());

        // iterate through the symbol table so we can find which sections are global (publicly visible)
        // we keep track of them here in a list
        let global_sections: BTreeSet<usize> = {
            // For us to properly load the ELF file, it must NOT have been stripped,
            // meaning that it must still have its symbol table section. Otherwise, relocations will not work.
            let symtab = find_symbol_table(&elf_file)?;

            let mut globals: BTreeSet<usize> = BTreeSet::new();
            use xmas_elf::symbol_table::Entry;
            for entry in symtab.iter() {
                // Previously we were ignoring "GLOBAL" symbols with "HIDDEN" visibility, but that excluded some symbols. 
                // So now, we include all symbols with "GLOBAL" binding, regardless of visibility.  
                if entry.get_binding() == Ok(xmas_elf::symbol_table::Binding::Global) {
                    if let Ok(typ) = entry.get_type() {
                        if typ == xmas_elf::symbol_table::Type::Func || typ == xmas_elf::symbol_table::Type::Object {
                            globals.insert(entry.shndx() as usize);
                        }
                    }
                }
            }
            globals 
        };


        let mut text_offset:   usize = 0;
        let mut rodata_offset: usize = 0;
        let mut data_offset:   usize = 0;
                    
        const TEXT_PREFIX:   &'static str = ".text.";
        const RODATA_PREFIX: &'static str = ".rodata.";
        const DATA_PREFIX:   &'static str = ".data.";
        const BSS_PREFIX:    &'static str = ".bss.";
        const RELRO_PREFIX:  &'static str = "rel.ro.";

        let new_crate = CowArc::new(LoadedCrate {
            crate_name:              String::from(crate_name),
            object_file:             object_file,
            sections:                BTreeMap::new(),
            text_pages:              text_pages  .as_ref().map(|r| Arc::clone(r)),
            rodata_pages:            rodata_pages.as_ref().map(|r| Arc::clone(r)),
            data_pages:              data_pages  .as_ref().map(|r| Arc::clone(r)),
            global_symbols:          BTreeSet::new(),
            bss_sections:            Trie::new(),
            reexported_symbols:      BTreeSet::new(),
        });
        let new_crate_weak_ref = CowArc::downgrade(&new_crate);
        
        // this maps section header index (shndx) to LoadedSection
        let mut loaded_sections: BTreeMap<usize, StrongSectionRef> = BTreeMap::new(); 
        // the list of all symbols in this crate that are public (global) 
        let mut global_symbols: BTreeSet<BString> = BTreeSet::new();
        // the map of BSS section names to the actual BSS section
        let mut bss_sections: Trie<BString, StrongSectionRef> = Trie::new();

        for (shndx, sec) in elf_file.section_iter().enumerate() {
            // the PROGBITS sections (.text, .rodata, .data) and the NOBITS (.bss) sections are what we care about
            let sec_typ = sec.get_type();
            // look for PROGBITS (.text, .rodata, .data) and NOBITS (.bss) sections
            if sec_typ == Ok(ShType::ProgBits) || sec_typ == Ok(ShType::NoBits) {

                // even if we're using the next section's data (for a zero-sized section),
                // we still want to use this current section's actual name and flags!
                let sec_flags = sec.flags();
                let sec_name = match sec.get_name(&elf_file) {
                    Ok(name) => name,
                    Err(_e) => {
                        error!("load_crate_sections: couldn't get section name for section [{}]: {:?}\n    error: {}", shndx, sec, _e);
                        return Err("couldn't get section name");
                    }
                };

                
                // some special sections are fine to ignore
                if  sec_name.starts_with(".note")   ||   // ignore GNU note sections
                    sec_name.starts_with(".gcc")    ||   // ignore gcc special sections for now
                    sec_name.starts_with(".debug")  ||   // ignore debug special sections for now
                    sec_name == ".text"                  // ignore the header .text section (with no content)
                {
                    continue;    
                }            


                let sec = if sec.size() == 0 {
                    // This is a very rare case of a zero-sized section. 
                    // A section of size zero shouldn't necessarily be removed, as they are sometimes referenced in relocations,
                    // typically the zero-sized section itself is a reference to the next section in the list of section headers).
                    // Thus, we need to use the *current* section's name with the *next* section's (the next section's) information,
                    // i.e., its  size, alignment, and actual data
                    match elf_file.section_header((shndx + 1) as u16) { // get the next section
                        Ok(sec_hdr) => {
                            // The next section must have the same offset as the current zero-sized one
                            if sec_hdr.offset() == sec.offset() {
                                // if it does, we can use it in place of the current section
                                sec_hdr
                            }
                            else {
                                // if it does not, we should NOT use it in place of the current section
                                sec
                            }
                        }
                        _ => {
                            error!("load_crate_sections(): Couldn't get next section for zero-sized section {}", shndx);
                            return Err("couldn't get next section for a zero-sized section");
                        }
                    }
                }
                else {
                    // this is the normal case, a non-zero sized section, so just use the current section
                    sec
                };

                // get the relevant section info, i.e., size, alignment, and data contents
                let sec_size  = sec.size()  as usize;
                let sec_align = sec.align() as usize;


                if sec_name.starts_with(TEXT_PREFIX) {
                    if let Some(name) = sec_name.get(TEXT_PREFIX.len() ..) {
                        let demangled = demangle(name).to_string();
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_EXECINSTR) {
                            error!(".text section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".text section had wrong flags!");
                        }

                        if let (Some(ref tp_ref), Some(ref mut tp)) = (&text_pages, &mut text_pages_locked) {
                            // here: we're ready to copy the text section to the proper address
                            let dest_addr = tp.address_at_offset(text_offset)
                                .ok_or_else(|| "BUG: text_offset wasn't within text_mapped_pages")?;
                            let dest_slice: &mut [u8]  = try!(tp.as_slice_mut(text_offset, sec_size));
                            match sec.get_data(&elf_file) {
                                Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                                Ok(SectionData::Empty) => {
                                    for b in dest_slice {
                                        *b = 0;
                                    }
                                },
                                _ => {
                                    error!("load_crate_sections(): Couldn't get section data for .text section [{}] {}: {:?}", shndx, sec_name, sec.get_data(&elf_file));
                                    return Err("couldn't get section data in .text section");
                                }
                            }

                            let is_global = global_sections.contains(&shndx);
                            if is_global {
                                global_symbols.insert(demangled.clone().into());
                            }

                            loaded_sections.insert(shndx, 
                                Arc::new(Mutex::new(LoadedSection::new(
                                    SectionType::Text,
                                    demangled,
                                    Arc::clone(tp_ref),
                                    text_offset,
                                    dest_addr,
                                    sec_size,
                                    is_global,
                                    new_crate_weak_ref.clone(),
                                )))
                            );

                            text_offset += round_up_power_of_two(sec_size, sec_align);
                        }
                        else {
                            return Err("no text_pages were allocated");
                        }
                    }
                    else {
                        error!("Failed to get the .text section's name after \".text.\": {:?}", sec_name);
                        return Err("Failed to get the .text section's name after \".text.\"!");
                    }
                }

                else if sec_name.starts_with(RODATA_PREFIX) {
                    if let Some(name) = sec_name.get(RODATA_PREFIX.len() ..) {
                        let demangled = demangle(name).to_string();
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC) {
                            error!(".rodata section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".rodata section had wrong flags!");
                        }

                        if let (Some(ref rp_ref), Some(ref mut rp)) = (&rodata_pages, &mut rodata_pages_locked) {
                            // here: we're ready to copy the rodata section to the proper address
                            let dest_addr = rp.address_at_offset(rodata_offset)
                                .ok_or_else(|| "BUG: rodata_offset wasn't within rodata_mapped_pages")?;
                            let dest_slice: &mut [u8]  = try!(rp.as_slice_mut(rodata_offset, sec_size));
                            match sec.get_data(&elf_file) {
                                Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                                Ok(SectionData::Empty) => {
                                    for b in dest_slice {
                                        *b = 0;
                                    }
                                },
                                _ => {
                                    error!("load_crate_sections(): Couldn't get section data for .rodata section [{}] {}: {:?}", shndx, sec_name, sec.get_data(&elf_file));
                                    return Err("couldn't get section data in .rodata section");
                                }
                            }

                            let is_global = global_sections.contains(&shndx);
                            if is_global {
                                global_symbols.insert(demangled.clone().into());
                            }
                            
                            loaded_sections.insert(shndx, 
                                Arc::new(Mutex::new(LoadedSection::new(
                                    SectionType::Rodata,
                                    demangled,
                                    Arc::clone(rp_ref),
                                    rodata_offset,
                                    dest_addr,
                                    sec_size,
                                    is_global,
                                    new_crate_weak_ref.clone(),
                                )))
                            );

                            rodata_offset += round_up_power_of_two(sec_size, sec_align);
                        }
                        else {
                            return Err("no rodata_pages were allocated");
                        }
                    }
                    else {
                        error!("Failed to get the .rodata section's name after \".rodata.\": {:?}", sec_name);
                        return Err("Failed to get the .rodata section's name after \".rodata.\"!");
                    }
                }

                else if sec_name.starts_with(DATA_PREFIX) {
                    if let Some(name) = sec_name.get(DATA_PREFIX.len() ..) {
                        let name = if name.starts_with(RELRO_PREFIX) {
                            let relro_name = try!(name.get(RELRO_PREFIX.len() ..).ok_or("Couldn't get name of .data.rel.ro. section"));
                            relro_name
                        }
                        else {
                            name
                        };
                        let demangled = demangle(name).to_string();
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_WRITE) {
                            error!(".data section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".data section had wrong flags!");
                        }
                        
                        if let (Some(ref dp_ref), Some(ref mut dp)) = (&data_pages, &mut data_pages_locked) {
                            // here: we're ready to copy the data/bss section to the proper address
                            let dest_addr = dp.address_at_offset(data_offset)
                                .ok_or_else(|| "BUG: data_offset wasn't within data_pages")?;
                            let dest_slice: &mut [u8]  = try!(dp.as_slice_mut(data_offset, sec_size));
                            match sec.get_data(&elf_file) {
                                Ok(SectionData::Undefined(sec_data)) => dest_slice.copy_from_slice(sec_data),
                                Ok(SectionData::Empty) => {
                                    for b in dest_slice {
                                        *b = 0;
                                    }
                                },
                                _ => {
                                    error!("load_crate_sections(): Couldn't get section data for .data section [{}] {}: {:?}", shndx, sec_name, sec.get_data(&elf_file));
                                    return Err("couldn't get section data in .data section");
                                }
                            }

                            let is_global = global_sections.contains(&shndx);
                            if is_global {
                                global_symbols.insert(demangled.clone().into());
                            }
                            
                            loaded_sections.insert(shndx, 
                                Arc::new(Mutex::new(LoadedSection::new(
                                    SectionType::Data,
                                    demangled,
                                    Arc::clone(dp_ref),
                                    data_offset,
                                    dest_addr,
                                    sec_size,
                                    is_global,
                                    new_crate_weak_ref.clone(),
                                )))
                            );

                            data_offset += round_up_power_of_two(sec_size, sec_align);
                        }
                        else {
                            return Err("no data_pages were allocated for .data section");
                        }
                    }
                    else {
                        error!("Failed to get the .data section's name after \".data.\": {:?}", sec_name);
                        return Err("Failed to get the .data section's name after \".data.\"!");
                    }
                }

                else if sec_name.starts_with(BSS_PREFIX) {
                    if let Some(name) = sec_name.get(BSS_PREFIX.len() ..) {
                        let demangled = demangle(name).to_string();
                        if sec_flags & (SHF_ALLOC | SHF_WRITE | SHF_EXECINSTR) != (SHF_ALLOC | SHF_WRITE) {
                            error!(".bss section [{}], name: {:?} had the wrong flags {:#X}", shndx, name, sec_flags);
                            return Err(".bss section had wrong flags!");
                        }
                        
                        // we still use DataSection to represent the .bss sections, since they have the same flags
                        if let (Some(ref dp_ref), Some(ref mut dp)) = (&data_pages, &mut data_pages_locked) {
                            // here: we're ready to fill the bss section with zeroes at the proper address
                            let dest_addr = dp.address_at_offset(data_offset)
                                .ok_or_else(|| "BUG: data_offset wasn't within data_pages")?;
                            let dest_slice: &mut [u8]  = try!(dp.as_slice_mut(data_offset, sec_size));
                            for b in dest_slice {
                                *b = 0;
                            };

                            let is_global = global_sections.contains(&shndx);
                            if is_global {
                                global_symbols.insert(demangled.clone().into());
                            }
                            
                            let sec_ref = Arc::new(Mutex::new(LoadedSection::new(
                                SectionType::Bss,
                                demangled.clone(),
                                Arc::clone(dp_ref),
                                data_offset,
                                dest_addr,
                                sec_size,
                                is_global,
                                new_crate_weak_ref.clone(),
                            )));
                            loaded_sections.insert(shndx, sec_ref.clone());
                            bss_sections.insert(demangled.into(), sec_ref);

                            data_offset += round_up_power_of_two(sec_size, sec_align);
                        }
                        else {
                            return Err("no data_pages were allocated for .bss section");
                        }
                    }
                    else {
                        error!("Failed to get the .bss section's name after \".bss.\": {:?}", sec_name);
                        return Err("Failed to get the .bss section's name after \".bss.\"!");
                    }
                }

                else {
                    error!("unhandled PROGBITS/NOBITS section [{}], name: {}, sec: {:?}", shndx, sec_name, sec);
                    continue;
                }

            }
        }

        // set the new_crate's section-related lists, since we didn't do it earlier
        {
            let mut new_crate_mut = new_crate.lock_as_mut()
                .ok_or_else(|| "BUG: load_crate_sections(): couldn't get exclusive mutable access to new_crate")?;
            new_crate_mut.sections = loaded_sections;
            new_crate_mut.global_symbols = global_symbols;
            new_crate_mut.bss_sections = bss_sections;
        }

        Ok((new_crate, elf_file))
    }

        
    /// The second stage of parsing and loading a new kernel crate, 
    /// filling in the missing relocation information in the already-loaded sections. 
    /// It also remaps the `new_crate`'s MappedPages according to each of their section permissions.
    fn perform_relocations(
        &self,
        elf_file: &ElfFile,
        new_crate_ref: &StrongCrateRef,
        backup_namespace: Option<&CrateNamespace>,
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool
    ) -> Result<(), &'static str> {
        let new_crate = new_crate_ref.lock_as_ref();
        if verbose_log { debug!("=========== moving on to the relocations for crate {} =========", new_crate.crate_name); }
        let symtab = find_symbol_table(&elf_file)?;

        // Fix up the sections that were just loaded, using proper relocation info.
        // Iterate over every non-zero relocation section in the file
        for sec in elf_file.section_iter().filter(|sec| sec.get_type() == Ok(ShType::Rela) && sec.size() != 0) {
            use xmas_elf::sections::SectionData::Rela64;
            use xmas_elf::symbol_table::Entry;
            if verbose_log { 
                trace!("Found Rela section name: {:?}, type: {:?}, target_sec_index: {:?}", 
                sec.get_name(&elf_file), sec.get_type(), sec.info()); 
            }

            // currently not using eh_frame, gcc, note, and debug sections
            if let Ok(name) = sec.get_name(&elf_file) {
                if  name.starts_with(".rela.eh_frame") || 
                    name.starts_with(".rela.note")     ||   // ignore GNU note sections
                    name.starts_with(".rela.gcc")      ||   // ignore gcc special sections for now
                    name.starts_with(".rela.debug")         // ignore debug special sections for now
                {
                    continue;
                }
            }

            let rela_array = match sec.get_data(&elf_file) {
                Ok(Rela64(rela_arr)) => rela_arr,
                _ => {
                    error!("Found Rela section that wasn't able to be parsed as Rela64: {:?}", sec);
                    return Err("Found Rela section that wasn't able to be parsed as Rela64");
                } 
            };

            // The target section is where we write the relocation data to.
            // The source section is where we get the data from. 
            // There is one target section per rela section (`rela_array`), and one source section per rela_entry in this rela section.
            // The "info" field in the Rela section specifies which section is the target of the relocation.
                
            // Get the target section (that we already loaded) for this rela_array Rela section.
            let target_sec_shndx = sec.info() as usize;
            let target_sec_ref = new_crate.sections.get(&target_sec_shndx).ok_or_else(|| {
                error!("ELF file error: target section was not loaded for Rela section {:?}!", sec.get_name(&elf_file));
                "target section was not loaded for Rela section"
            })?; 
            
            let mut target_sec_dependencies: Vec<StrongDependency> = Vec::new();
            let mut target_sec_internal_dependencies: Vec<InternalDependency> = Vec::new();

            let mut target_sec = target_sec_ref.lock();
            {
                let mut target_sec_mapped_pages = target_sec.mapped_pages.lock();

                // iterate through each relocation entry in the relocation array for the target_sec
                for rela_entry in rela_array {
                    if verbose_log { 
                        trace!("      Rela64 offset: {:#X}, addend: {:#X}, symtab_index: {}, type: {:#X}", 
                            rela_entry.get_offset(), rela_entry.get_addend(), rela_entry.get_symbol_table_index(), rela_entry.get_type());
                    }

                    let source_sec_entry: &Entry = &symtab[rela_entry.get_symbol_table_index() as usize];
                    let source_sec_shndx = source_sec_entry.shndx() as usize; 
                    if verbose_log { 
                        let source_sec_header_name = source_sec_entry.get_section_header(&elf_file, rela_entry.get_symbol_table_index() as usize)
                            .and_then(|s| s.get_name(&elf_file));
                        trace!("             relevant section [{}]: {:?}", source_sec_shndx, source_sec_header_name);
                        // trace!("             Entry name {} {:?} vis {:?} bind {:?} type {:?} shndx {} value {} size {}", 
                        //     source_sec_entry.name(), source_sec_entry.get_name(&elf_file), 
                        //     source_sec_entry.get_other(), source_sec_entry.get_binding(), source_sec_entry.get_type(), 
                        //     source_sec_entry.shndx(), source_sec_entry.value(), source_sec_entry.size());
                    }
                    
                    let mut source_and_target_in_same_crate = false;

                    // We first try to get the source section from loaded_sections, which works if the section is in the crate currently being loaded.
                    let source_sec_ref = match new_crate.sections.get(&source_sec_shndx) {
                        Some(ss) => {
                            source_and_target_in_same_crate = true;
                            Ok(ss.clone())
                        }

                        // If we couldn't get the section based on its shndx, it means that the source section wasn't in the crate currently being loaded.
                        // Thus, we must get the source section's name and check our list of foreign crates to see if it's there.
                        // At this point, there's no other way to search for the source section besides its name.
                        None => {
                            if let Ok(source_sec_name) = source_sec_entry.get_name(&elf_file) {
                                const DATARELRO: &'static str = ".data.rel.ro.";
                                let source_sec_name = if source_sec_name.starts_with(DATARELRO) {
                                    source_sec_name.get(DATARELRO.len() ..)
                                        .ok_or("Couldn't get name of .data.rel.ro. section")?
                                }
                                else {
                                    source_sec_name
                                };
                                let demangled = demangle(source_sec_name).to_string();

                                // search for the symbol's demangled name in the kernel's symbol map
                                self.get_symbol_or_load(&demangled, CrateType::Kernel.prefix(), backup_namespace, kernel_mmi, verbose_log)
                                    .upgrade()
                                    .ok_or("Couldn't get symbol for foreign relocation entry, nor load its containing crate")
                            }
                            else {
                                let _source_sec_header = source_sec_entry
                                    .get_section_header(&elf_file, rela_entry.get_symbol_table_index() as usize)
                                    .and_then(|s| s.get_name(&elf_file));
                                error!("Couldn't get name of source section [{}] {:?}, needed for non-local relocation entry", source_sec_shndx, _source_sec_header);
                                Err("Couldn't get source section's name, needed for non-local relocation entry")
                            }
                        }
                    }?;

                    let source_and_target_are_same_section = Arc::ptr_eq(&source_sec_ref, &target_sec_ref);
                    let relocation_entry = RelocationEntry::from_elf_relocation(rela_entry);

                    write_relocation(
                        relocation_entry,
                        &mut target_sec_mapped_pages,
                        target_sec.mapped_pages_offset,
                        if source_and_target_are_same_section { target_sec.virt_addr() } else { source_sec_ref.lock().virt_addr() },
                        verbose_log
                    )?;

                    if source_and_target_in_same_crate {
                        // We keep track of relocation information so that we can be aware of and faithfully reconstruct 
                        // inter-section dependencies even within the same crate.
                        // This is necessary for doing a deep copy of the crate in memory, 
                        // without having to re-parse that crate's ELF file (and requiring the ELF file to still exist)
                        target_sec_internal_dependencies.push(InternalDependency::new(relocation_entry, source_sec_shndx))
                    }
                    else {
                        // tell the source_sec that the target_sec is dependent upon it
                        let weak_dep = WeakDependent {
                            section: Arc::downgrade(&target_sec_ref),
                            relocation: relocation_entry,
                        };
                        source_sec_ref.lock().sections_dependent_on_me.push(weak_dep);
                        
                        // tell the target_sec that it has a strong dependency on the source_sec
                        let strong_dep = StrongDependency {
                            section: Arc::clone(&source_sec_ref),
                            relocation: relocation_entry,
                        };
                        target_sec_dependencies.push(strong_dep);          
                    }
                }
            }

            // add the target section's dependencies and relocation details all at once
            target_sec.sections_i_depend_on.append(&mut target_sec_dependencies);
            target_sec.internal_dependencies.append(&mut target_sec_internal_dependencies);
        }
        // here, we're done with handling all the relocations in this entire crate


        // Finally, remap each section's mapped pages to the proper permission bits, 
        // since we initially mapped them all as writable
        if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
            if let Some(ref tp) = new_crate.text_pages { 
                tp.lock().remap(active_table, TEXT_SECTION_FLAGS())?;
            }
            if let Some(ref rp) = new_crate.rodata_pages {
                rp.lock().remap(active_table, RODATA_SECTION_FLAGS())?;
            }
            // data/bss sections are already mapped properly, since they're supposed to be writable
        }
        else {
            return Err("couldn't get kernel's active page table");
        }

        Ok(())
    }

    
    /// Adds the given symbol to this namespace's symbol map.
    /// If the symbol already exists in the symbol map, this replaces the existing symbol with the new one, warning if they differ in size.
    /// Returns true if the symbol was added, and false if it already existed and thus was not added.
    fn add_symbol(
        existing_symbol_map: &mut SymbolMap,
        new_section_key: String,
        new_section_ref: &StrongSectionRef,
        log_replacements: bool,
    ) -> bool {
        match existing_symbol_map.entry(new_section_key.into()) {
            Entry::Occupied(_old_val) => {
                if log_replacements {
                    if let Some(old_sec_ref) = _old_val.get().upgrade() {
                        // debug!("       add_symbol(): replacing section: old: {:?}, new: {:?}", old_sec_ref, new_section_ref);
                        let old_sec = old_sec_ref.lock();
                        let new_sec = new_section_ref.lock();
                        if new_sec.size != old_sec.size {
                            warn!("       add_symbol(): Unexpectedly replacing differently-sized section: old: ({}B) {:?}, new: ({}B) {:?}", old_sec.size, old_sec.name, new_sec.size, new_sec.name);
                        } 
                        // else {
                        //     info!("       add_symbol(): Replacing new symbol already present: old {:?}, new: {:?}", old_sec.name, new_sec.name);
                        // }
                    }
                }
                false
            }
            Entry::Vacant(new_entry) => {
                new_entry.insert(Arc::downgrade(new_section_ref));
                true
            }
        }
    }

    /// Adds all global symbols in the given `sections` iterator to this namespace's symbol map. 
    /// 
    /// Returns the number of *new* unique symbols added.
    /// 
    /// # Note
    /// If a symbol already exists in the symbol map, this leaves the existing symbol intact and *does not* replace it.
    pub fn add_symbols<'a, I>(
        &self, 
        sections: I,
        _log_replacements: bool,
    ) -> usize
        where I: IntoIterator<Item = &'a StrongSectionRef>,
    {
        self.add_symbols_filtered(sections, |_sec| true, _log_replacements || true)
    }


    /// Adds symbols in the given `sections` iterator to this namespace's symbol map,
    /// but only the symbols that correspond to *global* sections AND for which the given `filter_func` returns true. 
    /// 
    /// Returns the number of *new* unique symbols added.
    /// 
    /// # Note
    /// If a symbol already exists in the symbol map, this leaves the existing symbol intact and *does not* replace it.
    fn add_symbols_filtered<'a, I, F>(
        &self, 
        sections: I,
        filter_func: F,
        log_replacements: bool,
    ) -> usize
        where I: IntoIterator<Item = &'a StrongSectionRef>,
              F: Fn(&LoadedSection) -> bool
    {
        let mut existing_map = self.symbol_map.lock();

        // add all the global symbols to the symbol map, in a way that lets us inspect/log each one
        let mut count = 0;
        for sec_ref in sections.into_iter() {
            let (sec_name, condition) = {
                let sec = sec_ref.lock();
                (
                    sec.name.clone(),
                    filter_func(&sec) && sec.global
                )
            };
            
            if condition {
                // trace!("add_symbols_filtered(): adding symbol {:?}", sec_ref);
                let added = CrateNamespace::add_symbol(&mut existing_map, sec_name, sec_ref, log_replacements);
                if added {
                    count += 1;
                }
            }
            // else {
            //     trace!("add_symbols_filtered(): skipping symbol {:?}", sec_ref);
            // }
        }
        
        count
    }


    /// A convenience function that returns a weak reference to the `LoadedSection`
    /// that matches the given name (`demangled_full_symbol`), if it exists in the symbol map.
    /// Otherwise, it returns None if the symbol does not exist.
    fn get_symbol_internal(&self, demangled_full_symbol: &str) -> Option<WeakSectionRef> {
        self.symbol_map.lock().get_str(demangled_full_symbol).cloned()
    }


    /// Finds the corresponding `LoadedSection` reference for the given fully-qualified symbol string.
    pub fn get_symbol(&self, demangled_full_symbol: &str) -> WeakSectionRef {
        self.get_symbol_internal(demangled_full_symbol)
            .unwrap_or_else(|| Weak::default())
    }


    /// Finds the corresponding `LoadedSection` reference for the given fully-qualified symbol string,
    /// similar to the simpler function `get_symbol()`.
    /// 
    /// If the symbol cannot be found in this namespace, it does the following:    
    /// (1) First, try to find the missing symbol in the `backup_namespace`. 
    ///     If we find it there, then add that shared crate into this namespace,
    ///     and add all of that shared crate's symbols into this crate as well. 
    /// 
    /// (2) Second, if the missing symbol isn't in the backup namespace either, 
    ///     try to load its containing crate from the module file. 
    ///     This can only be done for symbols that have a leading crate name, such as "my_crate::foo";
    ///     if a symbol was given the `no_mangle` attribute, then we will not be able to find it
    ///     and that symbol's containing crate should be manually loaded before invoking this. 
    /// 
    /// # Arguments
    /// * `demangled_full_symbol`: a fully-qualified symbol string, e.g., "my_crate::MyStruct::do_foo::h843a9ea794da0c24".
    /// * `kernel_crate_prefix`: the prefix string that goes in front of crate module names, 
    ///   which is generally "`k#`". 
    ///   You can specify the default by passing in `CrateType::Kernel.prefix()`, or specify another prefix
    ///   to help the symbol resolver know in which crate modules to look.
    /// * `backup_namespace`: the `CrateNamespace` that should be searched for missing symbols 
    ///   if a symbol cannot be found in this `CrateNamespace`. 
    ///   For example, the default namespace could be used by passing in `Some(get_default_namespace())`.
    ///   If `backup_namespace` is `None`, then no other namespace will be searched.
    /// * `kernel_mmi`: a mutable reference to the kernel's `MemoryManagementInfo`.
    pub fn get_symbol_or_load(
        &self, 
        demangled_full_symbol: &str, 
        kernel_crate_prefix: &str,
        backup_namespace: Option<&CrateNamespace>, 
        kernel_mmi: &mut MemoryManagementInfo,
        verbose_log: bool
    ) -> WeakSectionRef {
        // First, see if the section for the given symbol is already available and loaded
        if let Some(sec) = self.get_symbol_internal(demangled_full_symbol) {
            return sec;
        }

        // If not, our second try is to check the backup_namespace
        // to see if that namespace already has the section we want
        if let Some(backup) = backup_namespace {
            // info!("Symbol \"{}\" not initially found, attemping to load it from backup namespace {:?}", 
            //     demangled_full_symbol, backup.name);
            if let Some(weak_sec) = backup.get_symbol_internal(demangled_full_symbol) {
                if let Some(sec) = weak_sec.upgrade() {
                    // If we found it in the backup_namespace, then that saves us the effort of having to load the crate again.
                    // We need to add a shared reference to that section's parent crate to this namespace as well, 
                    // so it can't be dropped while this namespace is still relying on it.  
                    let pcref_opt = { sec.lock().parent_crate.upgrade() };
                    if let Some(parent_crate_ref) = pcref_opt {
                        let parent_crate_name = {
                            let parent_crate = parent_crate_ref.lock_as_ref();
                            // Here, there is a potential for optimization: add all symbols from the parent_crate into the current namespace.
                            // While this would save time if future symbols were needed from this crate,
                            // we *cannot* do this because it violates the expectations of certain namespaces. 
                            // For example, some namespaces may want to use just *one* symbol from another namespace's crate, not all of them. 
                            self.add_symbols(Some(sec.clone()).iter(), true);
                            parent_crate.crate_name.clone()
                        };
                        info!("Symbol {:?} not initially found, using symbol from (crate {:?}) in backup namespace {:?} in new namespace {:?}",
                            demangled_full_symbol, parent_crate_name, backup.name, self.name);
                        self.crate_tree.lock().insert(parent_crate_name.into(), parent_crate_ref);
                        return weak_sec;
                    }
                    else {
                        error!("get_symbol_or_load(): found symbol \"{}\" in backup namespace, but unexpectedly couldn't get its section's parent crate!",
                            demangled_full_symbol);
                        return Weak::default();
                    }
                }
            }
        }

        // If we couldn't get the symbol, then we attempt to load the kernel crate containing that symbol.
        // We are only able to do this for mangled symbols, those that have a leading crate name,
        // such as "my_crate::foo". 
        // If "foo()" was marked no_mangle, then we don't know which crate to load because there is no "my_crate::" before it.
        if let Some(crate_dependency_name) = get_containing_crate_name(demangled_full_symbol) {
            info!("Symbol \"{}\" not initially found, attemping to load its containing crate {:?}", 
                demangled_full_symbol, crate_dependency_name);
            
            // module names have a prefix like "k#", so we need to prepend that to the crate name
            let crate_dependency_name = format!("{}{}-", kernel_crate_prefix, crate_dependency_name);
 
            if let Some(dependency_module) = get_module_starting_with(&crate_dependency_name) {
                // try to load the missing symbol's containing crate
                if let Ok(_num_new_syms) = self.load_kernel_crate(dependency_module, backup_namespace, kernel_mmi, verbose_log) {
                    // try again to find the missing symbol, now that we've loaded the missing crate
                    if let Some(sec) = self.get_symbol_internal(demangled_full_symbol) {
                        return sec;
                    }
                    else {
                        error!("Symbol \"{}\" not found, even after loading its containing crate \"{}\". Is that symbol actually in the crate?", 
                            demangled_full_symbol, crate_dependency_name);                                                        
                    }
                }
                else {
                    error!("Found symbol's (\"{}\") containing crate, but couldn't load that crate module {:?}.",
                        demangled_full_symbol, dependency_module);
                }
            }
            else {
                error!("Symbol \"{}\" not found, and couldn't find its containing crate's module \"{}\".", 
                    demangled_full_symbol, crate_dependency_name);
            }
        }

        error!("Symbol \"{}\" not found. Try loading the crate manually first.", demangled_full_symbol);    
    
        Weak::default() // same as returning None, since it must be upgraded to an Arc before being used
    }


    /// Returns a copied list of the corresponding `LoadedSection`s 
    /// with names that start with the given `symbol_prefix`.
    /// 
    /// This method causes allocation because it creates a copy
    /// of the matching entries in the symbol map.
    /// 
    /// # Example
    /// The symbol map contains `my_crate::foo::h843a613894da0c24` and 
    /// `my_crate::foo::h933a635894ce0f12`. 
    /// Calling `find_symbols_starting_with("my_crate::foo")` will return 
    /// a vector containing both sections, which can then be iterated through.
    pub fn find_symbols_starting_with(&self, symbol_prefix: &str) -> Vec<(String, WeakSectionRef)> { 
        self.symbol_map.lock()
            .iter_prefix_str(symbol_prefix)
            .map(|(k, v)| (String::from(k.as_str()), v.clone()))
            .collect()
    }


    /// Returns a weak reference to the `LoadedSection` whose name beings with the given `symbol_prefix`,
    /// *if and only if* the symbol map only contains a single possible matching symbol.
    /// 
    /// # Important Usage Note
    /// To avoid greedily matching more symbols than expected, you may wish to end the `symbol_prefix` with "`::`".
    /// This may provide results more in line with the caller's expectations; see the last example below about a trailing "`::`". 
    /// This works because the delimiter between a symbol and its trailing hash value is "`::`".
    /// 
    /// # Example
    /// * The symbol map contains `my_crate::foo::h843a613894da0c24` 
    ///   and no other symbols that start with `my_crate::foo`. 
    ///   Calling `get_symbol_starting_with("my_crate::foo")` will return 
    ///   a weak reference to the section `my_crate::foo::h843a613894da0c24`.
    /// * The symbol map contains `my_crate::foo::h843a613894da0c24` and 
    ///   `my_crate::foo::h933a635894ce0f12`. 
    ///   Calling `get_symbol_starting_with("my_crate::foo")` will return 
    ///   an empty (default) weak reference, which is the same as returing None.
    /// * (Important) The symbol map contains `my_crate::foo::h843a613894da0c24` and 
    ///   `my_crate::foo_new::h933a635894ce0f12`. 
    ///   Calling `get_symbol_starting_with("my_crate::foo")` will return 
    ///   an empty (default) weak reference, which is the same as returing None,
    ///   because it will match both `foo` and `foo_new`. 
    ///   To match only `foo`, call this function as `get_symbol_starting_with("my_crate::foo::")`
    ///   (note the trailing "`::`").
    pub fn get_symbol_starting_with(&self, symbol_prefix: &str) -> WeakSectionRef { 
        let map = self.symbol_map.lock();
        let mut iter = map.iter_prefix_str(symbol_prefix).map(|tuple| tuple.1);
        iter.next()
            .filter(|_| iter.next().is_none()) // ensure single element
            .cloned()
            .unwrap_or_default()
    }

    
    /// Simple debugging function that returns the entire symbol map as a String.
    pub fn dump_symbol_map(&self) -> String {
        use core::fmt::Write;
        let mut output: String = String::new();
        let sysmap = self.symbol_map.lock();
        match write!(&mut output, "{:?}", sysmap.keys().collect::<Vec<_>>()) {
            Ok(_) => output,
            _ => String::from("(error)"),
        }
    }

}


/// Crate names must be only alphanumeric characters, an underscore, or a dash. 
/// See: <https://www.reddit.com/r/rust/comments/4rlom7/what_characters_are_allowed_in_a_crate_name/>
fn is_valid_crate_name_char(c: char) -> bool {
    char::is_alphanumeric(c) || 
    c == '_' || 
    c == '-'
}

/// Parses the given symbol string to try to find the contained parent crate
/// that contains the symbol. 
/// If the parent crate cannot be determined (e.g., a `no_mangle` symbol),
/// then the input string is returned unchanged.
/// # Examples
/// ```
/// <*const T as core::fmt::Debug>::fmt   -->  core 
/// <alloc::boxed::Box<T>>::into_unique   -->  alloc
/// keyboard::init                        -->  keyboard
/// ```
fn get_containing_crate_name<'a>(demangled_full_symbol: &'a str) -> Option<&'a str> {
    demangled_full_symbol.split("::").next().and_then(|s| {
        // Get the last word right before the first "::"
        s.rsplit(|c| !is_valid_crate_name_char(c))
            .next() // the first element of the iterator (last element before the "::")
    })
}


/// Similar to [`get_containing_crate_name()`](#method.get_containing_crate_name),
/// but replaces the parent crate name with the given `new_crate_name`, 
/// if it can be found, and if the parent crate name matches the `old_crate_name`. 
/// If the parent crate name can be found but does not match the expected `old_crate_name`,
/// then None is returned.
/// 
/// This creates an entirely new String rather than performing an in-place replacement, 
/// because the `new_crate_name` might be a different length than the original crate name.
/// # Examples
/// `keyboard::init  -->  keyboard_new::init`
fn replace_containing_crate_name<'a>(demangled_full_symbol: &'a str, old_crate_name: &str, new_crate_name: &str) -> Option<String> {
    // debug!("replace_containing_crate_name(dfs: {:?}, old: {:?}, new: {:?})", demangled_full_symbol, old_crate_name, new_crate_name);
    demangled_full_symbol.match_indices("::").next().and_then(|(index_of_first_double_colon, _substr)| {
        // Get the last word right before the first "::"
        demangled_full_symbol.get(.. index_of_first_double_colon).and_then(|substr| {
            // debug!("  replace_containing_crate_name(\"{}\"): substr: \"{}\" at index {}", demangled_full_symbol, substr, index_of_first_double_colon);
            let start_idx = substr.rmatch_indices(|c| !is_valid_crate_name_char(c))
                .next() // the first element right before the crate name starts
                .map(|(index_right_before_parent_crate_name, _substr)| { index_right_before_parent_crate_name + 1 }) // advance to the actual start 
                .unwrap_or(0); // if we didn't find any match, it means that everything since the beginning of the string was a valid char part of the parent_crate_name 

            demangled_full_symbol.get(start_idx .. index_of_first_double_colon)
                .filter(|&parent_crate_name| { parent_crate_name == old_crate_name })
                .and_then(|_parent_crate_name| {
                    // debug!("    replace_containing_crate_name(\"{}\"): parent_crate_name: \"{}\" at index [{}-{})", demangled_full_symbol, _parent_crate_name, start_idx, index_of_first_double_colon);
                    demangled_full_symbol.get(.. start_idx).and_then(|before_crate_name| {
                        demangled_full_symbol.get(index_of_first_double_colon ..).map(|after_crate_name| {
                            format!("{}{}{}", before_crate_name, new_crate_name, after_crate_name)
                        })
                    })
            })
        })
    })
}



/// A convenience wrapper for a set of the three possible types of `MappedPages`
/// that can be allocated and mapped for a single `LoadedCrate`. 
struct SectionPages {
    /// MappedPages that cover all .text sections, if any exist.
    text_pages:   Option<MappedPages>, //Option<Arc<Mutex<MappedPages>>>,
    /// MappedPages that cover all .rodata sections, if any exist.
    rodata_pages: Option<MappedPages>, //Option<Arc<Mutex<MappedPages>>>,
    /// MappedPages that cover all .data and .bss sections, if any exist.
    data_pages:   Option<MappedPages>, //Option<Arc<Mutex<MappedPages>>>,
}


/// Allocates enough space for the sections that are found in the given `ElfFile`.
/// Returns a tuple of `MappedPages` for the .text, .rodata, and .data/.bss sections, in that order.
fn allocate_section_pages(elf_file: &ElfFile, kernel_mmi: &mut MemoryManagementInfo) 
    -> Result<SectionPages, &'static str> 
{
    // Calculate how many bytes (and thus how many pages) we need for each of the three section types,
    // which are text (present | exec), rodata (present | noexec), data/bss (present | writable)
    let (text_bytecount, rodata_bytecount, data_bytecount): (usize, usize, usize) = {
        let (mut text, mut rodata, mut data) = (0, 0, 0);
        for sec in elf_file.section_iter() {
            let sec_typ = sec.get_type();
            // look for .text, .rodata, .data, and .bss sections
            if sec_typ == Ok(ShType::ProgBits) || sec_typ == Ok(ShType::NoBits) {
                let size = sec.size() as usize;
                if (size == 0) || (sec.flags() & SHF_ALLOC == 0) {
                    continue; // skip non-allocated sections (they're useless)
                }

                let align = sec.align() as usize;
                let addend = round_up_power_of_two(size, align);
    
                // filter flags for ones we care about (we already checked that it's loaded (SHF_ALLOC))
                let write: bool = sec.flags() & SHF_WRITE     == SHF_WRITE;
                let exec:  bool = sec.flags() & SHF_EXECINSTR == SHF_EXECINSTR;
                if exec {
                    // trace!("  Looking at sec with size {:#X} align {:#X} --> addend {:#X}", size, align, addend);
                    text += addend;
                }
                else if write {
                    // .bss sections have the same flags (write and alloc) as data, so combine them
                    data += addend;
                }
                else {
                    rodata += addend;
                }
            }
        }
        (text, rodata, data)
    };

    // create a closure here to allocate N contiguous virtual memory pages
    // and map them to random frames as writable, returns Result<MappedPages, &'static str>
    let (text_pages, rodata_pages, data_pages): (Option<MappedPages>, Option<MappedPages>, Option<MappedPages>) = {
        use memory::FRAME_ALLOCATOR;
        let mut frame_allocator = try!(FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME_ALLOCATOR")).lock();

        let mut allocate_pages_closure = |size_in_bytes: usize, flags: EntryFlags| {
            let allocated_pages = try!(allocate_pages_by_bytes(size_in_bytes).ok_or("Couldn't allocate_pages_by_bytes, out of virtual address space"));

            if let PageTable::Active(ref mut active_table) = kernel_mmi.page_table {
                // Right now we're just simply copying small sections to the new memory,
                // so we have to map those pages to real (randomly chosen) frames first. 
                // because we're copying bytes to the newly allocated pages, we need to make them writable too, 
                // and then change the page permissions (by using remap) later. 
                active_table.map_allocated_pages(allocated_pages, flags | EntryFlags::PRESENT | EntryFlags::WRITABLE, frame_allocator.deref_mut())
            }
            else {
                return Err("couldn't get kernel's active page table");
            }
    
        };

        // we must allocate these pages separately because they will have different flags later
        (
            if text_bytecount   > 0 { allocate_pages_closure(text_bytecount,   TEXT_SECTION_FLAGS()).ok()     } else { None }, 
            if rodata_bytecount > 0 { allocate_pages_closure(rodata_bytecount, RODATA_SECTION_FLAGS()).ok()   } else { None }, 
            if data_bytecount   > 0 { allocate_pages_closure(data_bytecount,   DATA_BSS_SECTION_FLAGS()).ok() } else { None }
        )
    };

    Ok(
        SectionPages {
            text_pages,
            rodata_pages,
            data_pages,
            // text_pages:   text_pages  .map(|tp| Arc::new(Mutex::new(tp))), 
            // rodata_pages: rodata_pages.map(|rp| Arc::new(Mutex::new(rp))),
            // data_pages:   data_pages  .map(|dp| Arc::new(Mutex::new(dp))),
        }
    )
}


/// Maps the given `ModuleArea` for a crate and returns the `MappedPages` that contain it. 
fn map_crate_module(crate_module: &ModuleArea, mmi: &mut MemoryManagementInfo) -> Result<MappedPages, &'static str> {
    use kernel_config::memory::address_is_page_aligned;
    if !address_is_page_aligned(crate_module.start_address()) {
        error!("map_crate_module(): crate_module {} is not page aligned!", crate_module.name());
        return Err("map_crate_module(): crate_module is not page aligned");
    } 

    // first we need to map the module memory region into our address space, 
    // so we can then parse the module as an ELF file in the kernel.
    if let PageTable::Active(ref mut active_table) = mmi.page_table {
        let new_pages = allocate_pages_by_bytes(crate_module.size()).ok_or("couldn't allocate pages for crate module")?;
        let mut frame_allocator = FRAME_ALLOCATOR.try().ok_or("couldn't get FRAME_ALLOCATOR")?.lock();
        active_table.map_allocated_pages_to(
            new_pages, 
            Frame::range_inclusive_addr(crate_module.start_address(), 
            crate_module.size()), 
            EntryFlags::PRESENT, 
            frame_allocator.deref_mut()
        )
    }
    else {
        error!("map_crate_module(): error getting kernel's active page table to temporarily map crate_module {}.", crate_module.name());
        Err("map_crate_module(): couldn't get kernel's active page table")
    }
}


/// Write the actual relocation entry.
/// # Arguments
/// * `rela_entry`: the relocation entry from the ELF file that specifies which relocation action to perform.
/// * `target_sec_mapped_pages`: the `MappedPages` that covers the target section, i.e., the section where the relocation data will be written to.
/// * `target_sec_mapped_pages_offset`: the offset into `target_sec_mapped_pages` where the target section is located.
/// * `source_sec`: the source section of the relocation, i.e., the section that the `target_sec` depends on and "points" to.
/// * `verbose_log`: whether to output verbose logging information about this relocation action.
fn write_relocation(
    relocation_entry: RelocationEntry,
    target_sec_mapped_pages: &mut MappedPages,
    target_sec_mapped_pages_offset: usize,
    source_sec_vaddr: VirtualAddress,
    verbose_log: bool
) -> Result<(), &'static str>
{
    // Calculate exactly where we should write the relocation data to.
    let target_offset = target_sec_mapped_pages_offset + relocation_entry.offset;

    // Perform the actual relocation data writing here.
    // There is a great, succint table of relocation types here
    // https://docs.rs/goblin/0.0.13/goblin/elf/reloc/index.html
    match relocation_entry.typ {
        R_X86_64_32 => {
            let target_ref: &mut u32 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u32;
        }
        R_X86_64_64 => {
            let target_ref: &mut u64 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u64;
        }
        R_X86_64_PC32 => {
            let target_ref: &mut u32 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend).wrapping_sub(target_ref as *mut _ as usize);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u32;
        }
        R_X86_64_PC64 => {
            let target_ref: &mut u64 = try!(target_sec_mapped_pages.as_type_mut(target_offset));
            let source_val = source_sec_vaddr.wrapping_add(relocation_entry.addend).wrapping_sub(target_ref as *mut _ as usize);
            if verbose_log { trace!("                    target_ptr: {:#X}, source_val: {:#X} (from sec_vaddr {:#X})", target_ref as *mut _ as usize, source_val, source_sec_vaddr); }
            *target_ref = source_val as u64;
        }
        // R_X86_64_GOTPCREL => { 
        //     unimplemented!(); // if we stop using the large code model, we need to create a Global Offset Table
        // }
        _ => {
            error!("found unsupported relocation type {}\n  --> Are you compiling crates with 'code-model=large'?", relocation_entry.typ);
            return Err("found unsupported relocation type. Are you compiling crates with 'code-model=large'?");
        }
    }

    Ok(())
}


#[allow(dead_code)]
fn dump_dependent_crates(krate: &LoadedCrate, prefix: String) {
	for weak_crate_ref in krate.crates_dependent_on_me() {
		let strong_crate_ref = weak_crate_ref.upgrade().unwrap();
        let strong_crate = strong_crate_ref.lock_as_ref();
		debug!("{}{}", prefix, strong_crate.crate_name);
		dump_dependent_crates(&*strong_crate, format!("{}  ", prefix));
	}
}


#[allow(dead_code)]
fn dump_weak_dependents(sec: &LoadedSection, prefix: String) {
	if !sec.sections_dependent_on_me.is_empty() {
		debug!("{}Section \"{}\": sections dependent on me (weak dependents):", prefix, sec.name);
		for weak_dep in &sec.sections_dependent_on_me {
			if let Some(wds) = weak_dep.section.upgrade() {
				let prefix = format!("{}  ", prefix); // add two spaces of indentation to the prefix
				dump_weak_dependents(&*wds.lock(), prefix);
			}
			else {
				debug!("{}ERROR: weak dependent failed to upgrade()", prefix);
			}
		}
	}
	else {
		debug!("{}Section \"{}\"  (no weak dependents)", prefix, sec.name);
	}
}




/// Returns a reference to the symbol table in the given `ElfFile`.
fn find_symbol_table<'e>(elf_file: &'e ElfFile) 
    -> Result<&'e [xmas_elf::symbol_table::Entry64], &'static str>
    {
    use xmas_elf::sections::SectionData::SymbolTable64;
    let symtab_data = elf_file.section_iter()
        .filter(|sec| sec.get_type() == Ok(ShType::SymTab))
        .next()
        .ok_or("no symtab section")
        .and_then(|s| s.get_data(&elf_file));

    match symtab_data {
        Ok(SymbolTable64(symtab)) => Ok(symtab),
        _ => {
            Err("no symbol table found. Was file stripped?")
        }
    }
}