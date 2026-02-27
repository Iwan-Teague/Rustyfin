'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import { WsPlayStateMessage, WsPresenceMember } from '@/lib/watchPartyApi';

type Props = {
  playState: WsPlayStateMessage | null;
  members: WsPresenceMember[];
  currentUserId: string;
  canControl: boolean;
  sendWs: (payload: Record<string, unknown>) => boolean;
};

const FILES = ['a', 'b', 'c', 'd', 'e', 'f', 'g', 'h'] as const;
const RANKS = ['8', '7', '6', '5', '4', '3', '2', '1'] as const;

const PIECE_SYMBOLS: Record<string, string> = {
  P: '♙',
  N: '♘',
  B: '♗',
  R: '♖',
  Q: '♕',
  K: '♔',
  p: '♟',
  n: '♞',
  b: '♝',
  r: '♜',
  q: '♛',
  k: '♚',
};

const PROMOTION_OPTIONS_WHITE = [
  { label: 'Queen', piece: 'Q', promotion: 'q' },
  { label: 'Rook', piece: 'R', promotion: 'r' },
  { label: 'Bishop', piece: 'B', promotion: 'b' },
  { label: 'Knight', piece: 'N', promotion: 'n' },
] as const;

const PROMOTION_OPTIONS_BLACK = [
  { label: 'Queen', piece: 'q', promotion: 'q' },
  { label: 'Rook', piece: 'r', promotion: 'r' },
  { label: 'Bishop', piece: 'b', promotion: 'b' },
  { label: 'Knight', piece: 'n', promotion: 'n' },
] as const;

function parseFenBoard(fen: string): Map<string, string> {
  const board = new Map<string, string>();
  const placement = fen.split(' ')[0] || '';
  const rows = placement.split('/');
  if (rows.length !== 8) return board;

  for (let rankIndex = 0; rankIndex < 8; rankIndex += 1) {
    const row = rows[rankIndex] || '';
    let fileIndex = 0;
    for (const token of row) {
      if (/\d/.test(token)) {
        fileIndex += Number(token);
        continue;
      }
      if (fileIndex > 7) continue;
      const square = `${FILES[fileIndex]}${8 - rankIndex}`;
      board.set(square, token);
      fileIndex += 1;
    }
  }

  return board;
}

function displayStatus(status: string): string {
  if (status === 'checkmate') return 'Checkmate';
  if (status === 'stalemate') return 'Stalemate';
  return 'Active';
}

function nameForUser(userId: string | null | undefined, members: WsPresenceMember[]): string {
  if (!userId) return 'Unassigned';
  return members.find((member) => member.user_id === userId)?.username ?? userId;
}

function pieceColor(piece: string): 'white' | 'black' {
  return piece === piece.toUpperCase() ? 'white' : 'black';
}

function randomSeatAssignment(
  members: WsPresenceMember[],
): { whiteUserId: string; blackUserId: string } | null {
  const connected = members.filter((member) => member.connected !== false);
  if (connected.length < 2) return null;
  const ids = connected.map((member) => member.user_id);
  const whiteIndex = Math.floor(Math.random() * ids.length);
  const whiteUserId = ids.splice(whiteIndex, 1)[0];
  const blackIndex = Math.floor(Math.random() * ids.length);
  const blackUserId = ids[blackIndex];
  if (!whiteUserId || !blackUserId || whiteUserId === blackUserId) {
    return null;
  }
  return { whiteUserId, blackUserId };
}

type PendingPromotion = {
  from: string;
  to: string;
  forColor: 'white' | 'black';
};

export default function PlayTogetherChess({
  playState,
  members,
  currentUserId,
  canControl,
  sendWs,
}: Props) {
  const [selectedSquare, setSelectedSquare] = useState<string | null>(null);
  const [pendingPromotion, setPendingPromotion] = useState<PendingPromotion | null>(null);
  const autoAssignedRef = useRef(false);

  const chess = playState?.chess;
  const board = useMemo(() => parseFenBoard(chess?.fen ?? ''), [chess?.fen]);
  const whiteUserId = chess?.white_user_id ?? null;
  const blackUserId = chess?.black_user_id ?? null;
  const turn = chess?.turn === 'black' ? 'black' : 'white';
  const myAssignedColor: 'white' | 'black' | null =
    whiteUserId === currentUserId ? 'white' : blackUserId === currentUserId ? 'black' : null;
  const turnUserId = turn === 'white' ? whiteUserId : blackUserId;
  const isMyTurnSeatAssigned = !!turnUserId && turnUserId === currentUserId;
  const canMove = canControl && isMyTurnSeatAssigned && chess?.status === 'active';
  const filesForDisplay =
    myAssignedColor === 'black' ? [...FILES].reverse() : [...FILES];
  const ranksForDisplay =
    myAssignedColor === 'black' ? [...RANKS].reverse() : [...RANKS];

  const selectedPiece = selectedSquare ? board.get(selectedSquare) : undefined;

  useEffect(() => {
    if (whiteUserId || blackUserId) {
      autoAssignedRef.current = false;
    }
  }, [whiteUserId, blackUserId]);

  useEffect(() => {
    if (!chess || !canControl) return;
    if (whiteUserId || blackUserId) return;
    if (autoAssignedRef.current) return;
    const assignment = randomSeatAssignment(members);
    if (!assignment) return;
    autoAssignedRef.current = true;
    sendWs({
      type: 'chess_set_players',
      white_user_id: assignment.whiteUserId,
      black_user_id: assignment.blackUserId,
    });
  }, [chess, canControl, whiteUserId, blackUserId, members, sendWs]);

  useEffect(() => {
    setSelectedSquare(null);
    setPendingPromotion(null);
  }, [chess?.fen]);

  function handleAssignPlayers(nextWhite: string | null, nextBlack: string | null) {
    if (!canControl) return;
    sendWs({
      type: 'chess_set_players',
      white_user_id: nextWhite || null,
      black_user_id: nextBlack || null,
    });
  }

  function handleSquareClick(square: string) {
    if (!chess || !canMove) return;
    const clickedPiece = board.get(square);

    if (!selectedSquare) {
      if (!clickedPiece) return;
      if (!myAssignedColor || pieceColor(clickedPiece) !== myAssignedColor) {
        return;
      }
      setSelectedSquare(square);
      return;
    }

    if (selectedSquare === square) {
      setSelectedSquare(null);
      return;
    }

    if (clickedPiece && myAssignedColor && pieceColor(clickedPiece) === myAssignedColor) {
      setSelectedSquare(square);
      return;
    }

    const isPawnPromotion =
      selectedPiece &&
      (selectedPiece === 'P' || selectedPiece === 'p') &&
      ((selectedPiece === 'P' && square.endsWith('8')) || (selectedPiece === 'p' && square.endsWith('1')));

    if (isPawnPromotion && myAssignedColor) {
      setPendingPromotion({
        from: selectedSquare,
        to: square,
        forColor: myAssignedColor,
      });
      return;
    }

    sendWs({
      type: 'chess_move',
      from: selectedSquare,
      to: square,
      promotion: null,
    });
    setSelectedSquare(null);
  }

  function applyPromotion(promotion: 'q' | 'r' | 'b' | 'n') {
    if (!pendingPromotion) return;
    sendWs({
      type: 'chess_move',
      from: pendingPromotion.from,
      to: pendingPromotion.to,
      promotion,
    });
    setPendingPromotion(null);
    setSelectedSquare(null);
  }

  if (!chess) {
    return (
      <section className="panel space-y-3 p-5 sm:p-6">
        <h2 className="text-xl font-semibold">Play Together</h2>
        <p className="text-sm muted">Connecting to shared game state…</p>
      </section>
    );
  }

  return (
    <section className="panel space-y-4 p-5 sm:p-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-end gap-2">
          <button
            type="button"
            className="rounded-t-lg border border-b-0 border-[var(--border)] bg-[var(--surface)] px-5 py-2.5 text-sm font-medium"
            onClick={() => sendWs({ type: 'play_set_game', game: 'chess' })}
            disabled={playState?.active_game === 'chess'}
          >
            Chess
          </button>
          <button
            type="button"
            className="rounded-t-lg border border-b-0 border-[var(--border)] px-5 py-2.5 text-sm font-medium opacity-50"
            disabled
          >
            More Soon
          </button>
        </div>
        <div className="ml-auto flex flex-wrap items-center justify-end gap-2">
          <span className="chip">Status: {displayStatus(chess.status)}</span>
          <span className="chip">Turn: {turn === 'white' ? 'White' : 'Black'}</span>
          {chess.winner_color && (
            <span className="chip">Winner: {chess.winner_color === 'white' ? 'White' : 'Black'}</span>
          )}
        </div>
      </div>

      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_18rem]">
        <div className="panel-soft rounded-xl p-3 sm:p-4">
          <div className="grid grid-cols-8 overflow-hidden rounded-lg border border-white/10">
            {ranksForDisplay.flatMap((rank, rankIndex) =>
              filesForDisplay.map((file, fileIndex) => {
                const square = `${file}${rank}`;
                const piece = board.get(square);
                const isLight = (rankIndex + fileIndex) % 2 === 0;
                const isSelected = selectedSquare === square;
                const isLastMove =
                  chess.last_move_from === square || chess.last_move_to === square;
                return (
                  <button
                    key={square}
                    type="button"
                    onClick={() => handleSquareClick(square)}
                    className={`relative flex h-12 items-center justify-center border border-white/5 text-2xl sm:h-14 ${
                      isLight ? 'bg-white/5' : 'bg-black/20'
                    } ${isSelected ? 'ring-2 ring-[var(--orange-soft)] ring-inset' : ''} ${
                      isLastMove ? 'outline outline-1 outline-[var(--purple)] outline-offset-[-1px]' : ''
                    }`}
                  >
                    <span className={piece && piece === piece.toLowerCase() ? 'text-white/90' : 'text-white'}>
                      {piece ? PIECE_SYMBOLS[piece] : ''}
                    </span>
                    <span className="pointer-events-none absolute bottom-0.5 right-1 text-[9px] text-white/35">
                      {square}
                    </span>
                  </button>
                );
              }),
            )}
          </div>
        </div>

        <aside className="panel-soft space-y-3 rounded-xl p-3 sm:p-4">
          <div className="space-y-1">
            <p className="text-xs uppercase tracking-wide muted">Players</p>
            <p className="text-xs muted">
              Assign white and black seats, or leave unassigned for open turns.
            </p>
          </div>

          <label className="block text-sm">
            <span className="mb-1 block text-xs uppercase tracking-wide muted">White</span>
            <select
              className="select px-2 py-2 text-sm"
              value={whiteUserId ?? ''}
              onChange={(event) => handleAssignPlayers(event.target.value || null, blackUserId)}
              disabled={!canControl}
            >
              <option value="">Unassigned</option>
              {members.map((member) => (
                <option key={member.user_id} value={member.user_id}>
                  {member.username}
                </option>
              ))}
            </select>
          </label>

          <label className="block text-sm">
            <span className="mb-1 block text-xs uppercase tracking-wide muted">Black</span>
            <select
              className="select px-2 py-2 text-sm"
              value={blackUserId ?? ''}
              onChange={(event) => handleAssignPlayers(whiteUserId, event.target.value || null)}
              disabled={!canControl}
            >
              <option value="">Unassigned</option>
              {members.map((member) => (
                <option key={member.user_id} value={member.user_id}>
                  {member.username}
                </option>
              ))}
            </select>
          </label>

          <div className="space-y-1 text-xs muted">
            <p>White: {nameForUser(whiteUserId, members)}</p>
            <p>Black: {nameForUser(blackUserId, members)}</p>
            <p>Your color: {myAssignedColor ? (myAssignedColor === 'white' ? 'White' : 'Black') : 'Unassigned'}</p>
            {turnUserId && <p>Current turn owner: {nameForUser(turnUserId, members)}</p>}
          </div>

          <button
            type="button"
            className="btn-secondary w-full px-3 py-2 text-sm"
            onClick={() => sendWs({ type: 'chess_reset' })}
            disabled={!canControl}
          >
            Reset Board
          </button>
        </aside>
      </div>
      {pendingPromotion && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4 backdrop-blur-[2px]">
          <div className="panel w-full max-w-sm space-y-4 rounded-2xl border border-[var(--border)] p-5">
            <h3 className="text-lg font-semibold">Choose Promotion Piece</h3>
            <p className="text-sm muted">Pick which piece this pawn should become.</p>
            <div className="grid grid-cols-2 gap-2">
              {(pendingPromotion.forColor === 'white'
                ? PROMOTION_OPTIONS_WHITE
                : PROMOTION_OPTIONS_BLACK
              ).map((option) => (
                <button
                  key={option.promotion}
                  type="button"
                  className="btn-secondary flex items-center justify-center gap-2 px-3 py-2 text-sm"
                  onClick={() => applyPromotion(option.promotion)}
                >
                  <span className="text-xl">{PIECE_SYMBOLS[option.piece]}</span>
                  <span>{option.label}</span>
                </button>
              ))}
            </div>
            <div className="flex justify-end">
              <button
                type="button"
                className="btn-ghost px-3 py-2 text-sm"
                onClick={() => setPendingPromotion(null)}
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
