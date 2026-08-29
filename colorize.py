import sys
from urllib.parse import urlparse
import hashlib
from datetime import datetime

RESET = '\033[0m'
BOLD  = '\033[1m'
DIM   = '\033[38;5;242m'
READ  = '\033[38;5;245m'

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

i = 0
for raw in sys.stdin:
    parts = raw.rstrip('\n').split('\x01')
    if len(parts) < 4:
        continue
    i += 1
    title, pubdate, url, unread = parts[0], parts[1], parts[2], parts[3]
    d = domain(url)
    label = tag(d)
    ago = time_ago(pubdate)
    num = '%2d.' % i

    if unread == '1':
        color = color_for(d)
        line1 = '%s%s%s %s%s%s %s(%s)%s' % (DIM, num, RESET, BOLD, title, RESET, DIM, d, RESET)
        line2 = '    %s%s%s%s · %s%s' % (color, label, RESET, DIM, ago, RESET)
    else:
        line1 = '%s%s  %s (%s)%s' % (READ, num, title, d, RESET)
        line2 = '%s    %s · %s%s' % (READ, label, ago, RESET)

    record = '%s\n%s\x01%s\x01%s\x01%s' % (line1, line2, pubdate, url, title)
    sys.stdout.write(record + '\0')

sys.stdout.flush()
