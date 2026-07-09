use core::alloc::{GlobalAlloc, Layout};
use core::mem;
use core::ptr::null_mut;
use spin::Mutex;

// ====================== FreeList ======================

/// Заголовок свободного блока. Хранится в памяти самого блока:
/// пока блок свободен, его первые 16 байт — это size + next.
struct FreeNode {
    size: usize,
    next: Option<&'static mut FreeNode>,
}

impl FreeNode {
    const fn new(size: usize) -> Self {
        Self { size, next: None }
    }

    /// Узел лежит в начале блока, поэтому адрес узла = адрес блока.
    fn start_addr(&self) -> usize {
        self as *const FreeNode as usize
    }

    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

pub struct FreeList {
    /// Фиктивный узел: size = 0, сам блоком не является.
    /// Нужен, чтобы вставка/удаление в начале списка не были спецслучаем.
    head: FreeNode,
}

impl FreeList {
    pub const fn new() -> Self {
        Self {
            head: FreeNode::new(0),
        }
    }

    /// # Safety
    /// [addr, addr+size) должна быть свободной памятью, отданной куче навсегда.
    pub unsafe fn init(&mut self, addr: usize, size: usize) {
        self.add_free_region(addr, size);
    }

    /// Вернуть регион в список свободных (вставка в голову).
    ///
    /// # Safety
    /// Регион никем не используется и не пересекается с уже свободными.
    unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
        // В блок должен влезать FreeNode, и адрес должен быть выровнен под него.
        // Ловим ошибку сразу, а не поехавшей кучей через тысячу аллокаций.
        assert_eq!(align_up(addr, mem::align_of::<FreeNode>()), addr);
        assert!(size >= mem::size_of::<FreeNode>());

        let mut node = FreeNode::new(size);
        // take(): переместить старую голову в next, оставив в head.next None —
        // &'static mut копировать нельзя, только передать во владение.
        node.next = self.head.next.take();
        let node_ptr = addr as *mut FreeNode;
        // write(), не `*node_ptr = node`: присваивание дропнуло бы «старое
        // значение» по адресу, а там неинициализированный мусор.
        node_ptr.write(node);
        self.head.next = Some(&mut *node_ptr);
    }

    /// First-fit: найти первый блок, куда влезает запрос, вынуть его из
    /// списка и вернуть вместе с выровненным адресом выдачи.
    fn find_region(&mut self, size: usize, align: usize) -> Option<(&'static mut FreeNode, usize)> {
        let mut current = &mut self.head;
        while let Some(ref mut region) = current.next {
            if let Ok(alloc_start) = Self::alloc_from_region(region, size, align) {
                // выкусить region из списка: current.next -> region.next
                let next = region.next.take();
                let ret = Some((current.next.take().unwrap(), alloc_start));
                current.next = next;
                return ret;
            } else {
                current = current.next.as_mut().unwrap();
            }
        }
        None
    }

    /// Влезает ли запрос (size, align) в данный блок?
    /// Ok(адрес выдачи) или Err, если блок не подходит.
    fn alloc_from_region(region: &FreeNode, size: usize, align: usize) -> Result<usize, ()> {
        let mut alloc_start = align_up(region.start_addr(), align);

        // Зазор спереди (из-за выравнивания) вернётся в список как
        // самостоятельный блок — значит, в него должен влезать FreeNode.
        // Если зазор слишком мал, сдвигаемся на следующую границу align.
        let front = alloc_start - region.start_addr();
        if front > 0 && front < mem::size_of::<FreeNode>() {
            alloc_start = align_up(region.start_addr() + mem::size_of::<FreeNode>(), align);
        }

        let alloc_end = alloc_start.checked_add(size).ok_or(())?;
        if alloc_end > region.end_addr() {
            return Err(());
        }

        // Хвост тоже вернётся в список: либо его нет, либо он вмещает FreeNode.
        let excess = region.end_addr() - alloc_end;
        if excess > 0 && excess < mem::size_of::<FreeNode>() {
            return Err(());
        }

        Ok(alloc_start)
    }

    /// size и align уже нормализованы через size_align().
    unsafe fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        match self.find_region(size, align) {
            Some((region, alloc_start)) => {
                // Снимаем границы в локальные переменные: память узла сейчас
                // будет перезаписана (зазором спереди или данными пользователя).
                let region_start = region.start_addr();
                let region_end = region.end_addr();
                let alloc_end = alloc_start + size;

                let front = alloc_start - region_start;
                if front > 0 {
                    self.add_free_region(region_start, front);
                }
                let excess = region_end - alloc_end;
                if excess > 0 {
                    self.add_free_region(alloc_end, excess);
                }
                alloc_start as *mut u8
            }
            None => null_mut(), // OOM
        }
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8, size: usize) {
        self.add_free_region(ptr as usize, size);
    }
}

/// Нормализовать layout: любой выданный блок обязан при освобождении
/// уметь стать FreeNode — значит, минимум 16 байт и выравнивание >= 8.
/// pad_to_align() держит размер кратным выравниванию, чтобы соседние
/// свободные куски не теряли выравнивание под FreeNode.
fn size_align(layout: Layout) -> (usize, usize) {
    let layout = layout
        .align_to(mem::align_of::<FreeNode>())
        .expect("align_to failed")
        .pad_to_align();
    let size = layout.size().max(mem::size_of::<FreeNode>());
    (size, layout.align())
}

fn align_up(addr: usize, align: usize) -> usize {
    if align <= 1 {
        return addr;
    }
    (addr + align - 1) & !(align - 1)
}

// ====================== Global Allocator ======================

pub struct LockedHeap {
    inner: Mutex<FreeList>,
}

impl LockedHeap {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(FreeList::new()),
        }
    }

    pub fn init(&self, addr: u64, size: u64) {
        self.with_lock(|fl| unsafe { fl.init(addr as usize, size as usize) });
    }

    /// Лок кучи с выключенными прерываниями. Если обработчик прерывания
    /// попробует аллоцировать, пока ядро держит этот спинлок, — дедлок;
    /// поэтому на время лока гасим IF и восстанавливаем как было.
    fn with_lock<R>(&self, f: impl FnOnce(&mut FreeList) -> R) -> R {
        let rflags: u64;
        unsafe {
            core::arch::asm!("pushfq; pop {}; cli", out(reg) rflags, options(nomem));
        }
        let ret = {
            let mut fl = self.inner.lock();
            f(&mut fl)
        };
        if rflags & (1 << 9) != 0 {
            unsafe { core::arch::asm!("sti", options(nomem, nostack)) };
        }
        ret
    }
}

#[global_allocator]
pub static HEAP: LockedHeap = LockedHeap::new();

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let (size, align) = size_align(layout);
        self.with_lock(|fl| unsafe { fl.alloc(size, align) })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Тот же size_align, что и при alloc: освобождаем ровно то, что выдали.
        let (size, _) = size_align(layout);
        self.with_lock(|fl| unsafe { fl.dealloc(ptr, size) })
    }
}

// ====================== Удобные функции ======================

/// Прогон кучи при загрузке. Суммарно просит в разы больше памяти, чем
/// есть в куче (16 МиБ через 1 МиБ), — выживает, только если dealloc
/// реально возвращает блоки в список. Падение = паника с file:line.
pub fn self_test() {
    use alloc::boxed::Box;
    use alloc::vec::Vec;

    // 1) churn: bump-аллокатор умер бы на ~256-й итерации
    for i in 0..4096u64 {
        let mut v: Vec<u8> = Vec::with_capacity(4096);
        v.push(i as u8);
        assert_eq!(v[0], i as u8);
    }

    // 2) много мелких блоков вперемешку + проверка, что данные не побились
    for round in 0..64u64 {
        let boxes: Vec<Box<u64>> = (0..64).map(|i| Box::new(round * 1000 + i)).collect();
        for (i, b) in boxes.iter().enumerate() {
            assert_eq!(**b, round * 1000 + i as u64);
        }
    }

    // 3) выравнивание
    let p = kmalloc_aligned(64, 4096);
    assert!(!p.is_null());
    assert_eq!(p as usize % 4096, 0);
}

pub fn kmalloc(size: usize) -> *mut u8 {
    kmalloc_aligned(size, 8)
}

pub fn kmalloc_aligned(size: usize, align: usize) -> *mut u8 {
    match Layout::from_size_align(size, align) {
        Ok(layout) => unsafe { HEAP.alloc(layout) },
        Err(_) => null_mut(),
    }
}
