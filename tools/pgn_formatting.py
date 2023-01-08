import requests
from pgn_parser import pgn, parser
import chess.pgn
import chess.engine
import re

def get_from_moves(moves):
    game_txt = ''
    i = 0
    n = 1
    for m in moves:
        if (i % 2) == 0:
            game_txt = game_txt + str(n) + '.' + m[1] + ' '
            n = n + 1
        i = i + 1   
    return game_txt

# get pgn from api or other way
# pgn by andrew standart
def get_moves_from_bughouse_pgn(txt):
    # find line started woth number
    # other started with '['
    lines = txt.split('\n')
    i = 0
    for line in lines:
        if line[0] == '1':
            break
    moves_start = re.search(line, txt).start()
    
    # moves from 2 boards: A(a for black) and B (b for black)
    moves_2b = txt[moves_start:]
    # clear simbols
    moves_2b = moves_2b.replace('\n', '')
    
    # lets start woth regex
    # simbols between nA. and spaces
    patterna = '\\d(A|a). (.*?)\ '
    patternb = '\\d(B|b). (.*?)\ '
    moves_a = re.findall(patterna, moves_2b)
    moves_b = re.findall(patternb, moves_2b)
    
    pgn_a = get_from_moves(moves_a)
    pgn_b = get_from_moves(moves_b)
    
    return pgn_a, pgn_b

# for tests:
# r = requests.get('http://bughouse.pro/dyn/pgn/250')
# bug = r.text
# pgn_a, pgn_b = get_moves_from_bughouse_pgn(bug)