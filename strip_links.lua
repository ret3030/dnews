function Link(el) return el.content end
function Image() return {} end
local h1_done = false
function Header(el)
    if el.level == 1 and not h1_done then h1_done = true; return {} end
    return el
end
