# coding: utf-8
# наше всё
import numpy as np
import pandas as pd

import requests

# start rating value
start_rating = 1600
# start of id increment
# id you have a good in sql, just forget
match_id = 1000

# функция пытается ватщить число из строки
# txt - входная строка
# пытаемся найти число между паттернами
def parce_number(txt, start_pattern, finish_pattern):
    res = 0
    
    try:
        # нам нужно правее первого патерна
        right_txt = txt.split(start_pattern)[1]
        # и левее правого
        res_txt = right_txt.split(finish_pattern)[0]
    except Exception as e:
        # если не нашлось, вернём пустую строку
        res_txt = ''
        res = 0
    
    # если можем, делаем число
    try:
        res_txt = res_txt.replace('"', '')
        res = float(res_txt)
    except Exception as e:
        # если не шмогла, то оставляем как есть
        res = res_txt
#         print(str(e))
    
    return res


# get params from pgn
# according structure
def take_game_params(pgn):
    game_params = []
    game_params.append(parce_number(pgn, "WhiteA ", "]"))
    game_params.append(parce_number(pgn, "WhiteB ", "]"))
    game_params.append(parce_number(pgn, "BlackA ", "]"))
    game_params.append(parce_number(pgn, "BlackB ", "]"))
    game_params.append(parce_number(pgn, "UTCDate ", "]"))
    game_params.append(parce_number(pgn, "Round ", "]"))
    game_params.append(parce_number(pgn, "Result ", "]"))
    game_params.append(parce_number(pgn, "Termination ", "]"))
    game_params.append(parce_number(pgn, "Outcome ", "]"))
    
    return game_params

# return classic Elo propabilities
# elo_prob(2882, 2722) -> 0.7152 (72% chanses Carlsen (2882) to beat Wan Hao (2722))
def elo_prob(rw, rb):
    try:
        rw=float(rw)
        rb=float(rb)
        res=1/(1+np.power(10, (rb-rw)/400))
    except:
        0.5
    return res

# rating changing after game
# elo_rating_changes(1600, 1200, 0.5)
def elo_rating_changes(rating, opponent_rating, score):
    K = 20
    # fast tunnel for newcomers
#     if games<=30:
#         K=40
#     else:
#         # slow tunnel for tops
#         if rating>2500:
#             K=10
#         elif rating<=2500:
#             K=20
            
    expectation=elo_prob(rating, opponent_rating)
    new_rating=rating+K*(score-expectation)
    
    return np.round(new_rating,2)

# get kind of results from outcome
def parce_outcome(txt):
    vrs = txt.split('by ')
    if len(vrs) > 1:
        res = vrs[1]
    else:
        res = 'other'
    
    return res

# teams with 2 players with sorting
def make_team(l1, l2):
    l = []
    l.append(l1)
    l.append(l2)
    l.sort()
    txt = str(l[0]) + ',' + str(l[1])
    
    return txt

# make df from params list of lists
def get_df(all_params):
    df = pd.DataFrame(all_params)
    df.columns = [
        'WhiteA',
        'WhiteB',
        'BlackA',
        'BlackB',
        'UTCDate',
        'Round',
        'Result',
        'Termination',
        'Outcome',
        'GameIndex'
    ]
    df['Round'] = df['Round'].astype('int')
    df['Outcome'] = np.where(
                            df['Outcome']=='', df['Termination'], df['Outcome']
                            )
    df['Reason'] = df['Outcome'].apply(parce_outcome)
    # teams
    df['Red'] = df[['WhiteA','BlackB']].apply(lambda x: make_team(x[0], x[1]), axis=1)
    df['Blue'] = df[['WhiteB','BlackA']].apply(lambda x: make_team(x[0], x[1]), axis=1)
    df['ResultRed'] = df['Result'].apply(lambda x: x.split('-')[0]).astype('int')
    df['ResultBlue'] = df['Result'].apply(lambda x: x.split('-')[1]).astype('int')
    
    return df


# get data from host
# if you have sql, it's no need
# list of params list
all_params = []
# one call - one game
for i in range(1,5000):
    url='http://bughouse.pro/dyn/pgn/' + str(i)
    print(url)
    r = requests.get(url)
    if r.status_code == 200:
        gp = take_game_params(r.text)
        # adding game index
        gp.append(i)
        all_params.append(gp)
        gp = []
    else:
        break
        
df = get_df(all_params)

# ELO rating countings
# dict with current rating
rating_dct = {}
red_rating_lst = []
blue_rating_lst = []
match_id_lst = []
for i in range(len(df)):
    # matchid as a bonus
    if df['Round'].values[i] == 1:
        match_id = match_id + 1
    
    team = df['Red'].values[i]
    opponent = df['Blue'].values[i]
    score = df['ResultRed'].values[i]
    opponent_score = df['ResultBlue'].values[i]
    
    # TO DO: use default dict
    # current ratings always in dict
    # history in lists
    try:
        rating = rating_dct[team]
    except:
        rating = start_rating
        rating_dct.update({team: rating})
    try:
        opponent_rating = rating_dct[opponent]
    except:
        opponent_rating = start_rating
        rating_dct.update({opponent: opponent_rating})
    
    # both ratings count simultaniously 
    rating =  elo_rating_changes(rating, opponent_rating, score)
    opponent_rating =  elo_rating_changes(opponent_rating, rating, opponent_score)
    rating_dct.update({team: rating})
    rating_dct.update({opponent: opponent_rating})
    # and list for dataframe
    red_rating_lst.append(rating)
    blue_rating_lst.append(opponent_rating)
    match_id_lst.append(match_id)
df['RatingRed'] = red_rating_lst
df['RatingBlue'] = blue_rating_lst
df['MatchID'] = match_id_lst
df['GameID'] = (df['GameIndex'].astype('str') + '-' + 
    df['MatchID'].astype('str') + '-' + df['Round'].astype('str'))


# data for players
# doubles, but good orientation
p1 = df.copy()
p1['Player'] = df['WhiteA']
p1['Color'] = 'White'
p1['Team'] = df['Red']
p1['Opponent'] = df['BlackA']
p1['Score'] = df['ResultRed']
p1['Rating'] = df['RatingRed']
p1['GameID'] = df['GameID']
p1 = p1[['Player', 'Team', 'Color', 'Opponent', 'Score', 'Rating', 'GameID']]

p2 = df.copy()
p2['Player'] = df['BlackB']
p2['Color'] = 'Black'
p2['Team'] = df['Red']
p2['Opponent'] = df['WhiteB']
p2['Score'] = df['ResultRed']
p2['Rating'] = df['RatingRed']
p2['GameID'] = df['GameID']
p2 = p2[['Player', 'Team', 'Color', 'Opponent', 'Score', 'Rating', 'GameID']]

p3 = df.copy()
p3['Player'] = df['BlackA']
p3['Color'] = 'Black'
p3['Team'] = df['Blue']
p3['Opponent'] = df['WhiteA']
p3['Score'] = df['ResultBlue']
p3['Rating'] = df['RatingBlue']
p3['GameID'] = df['GameID']
p3 = p3[['Player', 'Team', 'Color', 'Opponent', 'Score', 'Rating', 'GameID']]

p4 = df.copy()
p4['Player'] = df['WhiteB']
p4['Color'] = 'White'
p4['Team'] = df['Blue']
p4['Opponent'] = df['BlackB']
p4['Score'] = df['ResultBlue']
p4['Rating'] = df['RatingBlue']
p4['GameID'] = df['GameID']
p4 = p4[['Player', 'Team', 'Color', 'Opponent', 'Score', 'Rating', 'GameID']]

player_base_df = pd.concat([p1,p2,p3, p4])
player_base_df['ResultID'] = np.arange(len(player_base_df))

# game data
# if this data will be in SQl it would be perfect
player_base_df = player_base_df.merge(
    df[['GameID', 'MatchID', 'GameIndex', 'Round', 'UTCDate', 'Outcome', 'Reason']],
    'left',
    on=['GameID']
)
player_base_df = player_base_df.sort_values(by='GameIndex')

team_stat = player_base_df.groupby('Team').agg(
                {
                    'MatchID': lambda x: x.nunique(),
                    'GameID': lambda x: x.nunique(),
                    'Score': np.sum
                }).reset_index()
# two players in team
team_stat['Score'] = team_stat['Score'] / 2 
team_stat['WinRate'] = team_stat['Score'] / team_stat['GameID']
team_stat['Rating'] = team_stat['Team'].map(rating_dct)

team_stat = team_stat.sort_values(by=['WinRate', 'GameID'], ascending=[False, False])

player_stat = player_base_df.groupby('Player').agg(
                {
                    'MatchID': lambda x: x.nunique(),
                    'GameID': lambda x: x.nunique(),
                    'Team': lambda x: x.nunique(),
                    'Score': np.sum
                }).reset_index()

player_stat['Score'] = player_stat['Score']
player_stat['WinRate'] = player_stat['Score'] / player_stat['GameID']


player_stat = player_stat.sort_values(by=['WinRate', 'GameID'], ascending=[False, False])