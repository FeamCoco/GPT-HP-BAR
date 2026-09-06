"""生成 GPT-HP-BAR 图标：圆角方块 绿→蓝渐变 + 白色血条（仅标准库）"""
import struct, zlib, os

def png(w, h, rgba):
    def chunk(typ, data):
        return struct.pack('>I', len(data)) + typ + data + struct.pack('>I', zlib.crc32(typ + data) & 0xffffffff)
    raw = b''.join(b'\x00' + bytes(rgba[y*w*4:(y+1)*w*4]) for y in range(h))
    return (b'\x89PNG\r\n\x1a\n'
            + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(raw, 9))
            + chunk(b'IEND', b''))

def draw(S):
    px = [0] * (S * S * 4)
    x0, y0, x1, y1 = S*0.06, S*0.06, S*0.94, S*0.94
    r = S * 0.22
    for y in range(S):
        for x in range(S):
            i = (y*S + x) * 4
            cx, cy = x + 0.5, y + 0.5
            inside = x0 <= cx <= x1 and y0 <= cy <= y1
            if inside:
                qx, qy = min(cx-x0, x1-cx), min(cy-y0, y1-cy)
                if qx < r and qy < r:
                    dx, dy = r-qx, r-qy
                    if dx*dx + dy*dy > r*r:
                        inside = False
            if inside:
                t = (x - x0) / (x1 - x0)
                px[i]   = int(16 + (14 - 16) * t)
                px[i+1] = int(185 - (185-120) * t)   # 绿 → 青
                px[i+2] = int(129 + (232-129) * t)   # → 蓝
                px[i+3] = 255
    # 白色血条
    bx0, bx1 = int(S*0.16), int(S*0.84)
    by0, by1 = int(S*0.42), int(S*0.58)
    for y in range(by0, by1):
        for x in range(bx0, bx1):
            i = (y*S + x) * 4
            px[i], px[i+1], px[i+2], px[i+3] = 255, 255, 255, 235
    # 血条右端留白缺口示意 73%
    nx0 = int(bx0 + (bx1-bx0) * 0.73)
    for y in range(by0+2, by1-2):
        for x in range(nx0, bx1-1):
            i = (y*S + x) * 4
            px[i+3] = 70
    return px

os.makedirs(os.path.join(os.path.dirname(__file__) or '.', 'icons'), exist_ok=True)
base = os.path.dirname(__file__) or '.'
for S, name in [(32, '32x32.png'), (128, '128x128.png'), (256, '256x256.png')]:
    with open(os.path.join(base, 'icons', name), 'wb') as f:
        f.write(png(S, S, draw(S)))
    print('write', name)

ico_png = open(os.path.join(base, 'icons', '256x256.png'), 'rb').read()
ico = struct.pack('<HHH', 0, 1, 1) + struct.pack('<BBBBHHII', 0, 0, 0, 0, 1, 32, len(ico_png), 22) + ico_png
with open(os.path.join(base, 'icons', 'icon.ico'), 'wb') as f:
    f.write(ico)
print('write icon.ico')
