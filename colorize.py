import sys
from urllib.parse import urlparse
import hashlib
from datetime import datetime

RESET  = '\033[0m'
DIM    = '\033[38;5;242m'

def domain(u):
    try: return urlparse(u).netloc.replace('www.', '')
    except: return ''

def color_for(d):
    palette = [166, 108, 132, 72, 172, 167, 66, 142]
    idx = int(hashlib.md5(d.encode()).hexdigest(), 16) % len(palette)
    return '\033[38;5;%dm' % palette[idx]

def tag(d):
    name = d.split('.')[0].upper()
    vowels = 'AEIOU'
    consonants = ''.join(c for c in name if c not in vowels)
    return (consonants[:3] if len(consonants) >= 3 else name[:3]).ljust(3)

def time_ago(pubdate):
    try:
        diff = datetime.now() - datetime.fromtimestamp(int(pubdate))
        s = int(diff.total_seconds())
        if s < 3600: return '%dm ago' % (s // 60)
        if s < 86400: return '%dh ago' % (s // 3600)
        return '%dd ago' % (s // 86400)
    except: return ''

lines = [l.rstrip() for l in sys.stdin]
parsed = []
for line in lines:
    parts = line.split('\x01')
    if len(parts) < 3: continue
    parsed.append((parts[0], parts[1], parts[2]))

for i, (title, pubdate, url) in enumerate(parsed, 1):
    d = domain(url)
    label = tag(d)
    color = color_for(d)
    ago = time_ago(pubdate)
    num = '%2d.' % i
    print('%s%s%s  %s%s%s  %s%s · %s · %s%s\x01%s\x01%s' % (
        DIM, num, RESET,
        color, label, RESET,
        title,
        DIM, ago, d, RESET,
        pubdate, url
    ))
